//! Multi-cloud deployment support, scoped honestly: this generates
//! ready-to-edit Terraform/cloud-init scaffolding for standing up a
//! Monolith node on a given provider. It does not hold cloud credentials
//! or call any provider API — `terraform apply` (or the provider's own
//! CLI) is still the thing that actually provisions anything. That's a
//! deliberate boundary: a bundled multi-cloud *deployer* would mean
//! mnctl handling real cloud credentials, which is a much bigger trust
//! and blast-radius commitment than "generate the files you'd write
//! yourself anyway".
use anyhow::{Context, Result};
use clap::{Args, Subcommand, ValueEnum};
use colored::Colorize;

#[derive(Args)]
pub struct CloudArgs {
    #[command(subcommand)]
    command: CloudCommand,
}

#[derive(Subcommand)]
enum CloudCommand {
    /// Generate deployment scaffolding for a cloud provider
    Template {
        #[arg(long, value_enum)]
        provider: Provider,
        /// Output directory
        #[arg(long, default_value = "./monolith-cloud")]
        out: String,
        /// Node hostname to bake into cloud-init
        #[arg(long, default_value = "monolith-node")]
        hostname: String,
    },
}

#[derive(Clone, ValueEnum)]
enum Provider {
    Hetzner,
    Digitalocean,
    Aws,
}

impl CloudArgs {
    pub async fn run(self) -> Result<()> {
        match self.command {
            CloudCommand::Template {
                provider,
                out,
                hostname,
            } => cloud_template(&provider, &out, &hostname),
        }
    }
}

fn cloud_init(hostname: &str) -> String {
    format!(
        "#cloud-config\n\
         hostname: {hostname}\n\
         package_update: true\n\
         runcmd:\n\
         \x20 # Monolith's own bootstrap installer takes it from here —\n\
         \x20 # this cloud-init only needs to get curl and network up.\n\
         \x20 - curl -fsSL https://raw.githubusercontent.com/shirou-eh/Monolith/main/scripts/bootstrap.sh | bash\n"
    )
}

fn cloud_template(provider: &Provider, out: &str, hostname: &str) -> Result<()> {
    std::fs::create_dir_all(out).with_context(|| format!("failed to create {out}"))?;

    let init = cloud_init(hostname);
    std::fs::write(format!("{out}/cloud-init.yaml"), &init)?;

    let (tf, notes) = match provider {
        Provider::Hetzner => (
            format!(
                "terraform {{\n\
                 \x20 required_providers {{\n\
                 \x20   hcloud = {{ source = \"hetznercloud/hcloud\" }}\n\
                 \x20 }}\n\
                 }}\n\n\
                 variable \"hcloud_token\" {{}}\n\n\
                 provider \"hcloud\" {{\n\
                 \x20 token = var.hcloud_token\n\
                 }}\n\n\
                 resource \"hcloud_server\" \"{hostname}\" {{\n\
                 \x20 name        = \"{hostname}\"\n\
                 \x20 server_type = \"cx22\"\n\
                 \x20 image       = \"archlinux\"\n\
                 \x20 location    = \"nbg1\"\n\
                 \x20 user_data   = file(\"cloud-init.yaml\")\n\
                 }}\n\n\
                 output \"ipv4\" {{\n\
                 \x20 value = hcloud_server.{hostname}.ipv4_address\n\
                 }}\n"
            ),
            "export TF_VAR_hcloud_token=<your Hetzner API token>",
        ),
        Provider::Digitalocean => (
            format!(
                "terraform {{\n\
                 \x20 required_providers {{\n\
                 \x20   digitalocean = {{ source = \"digitalocean/digitalocean\" }}\n\
                 \x20 }}\n\
                 }}\n\n\
                 variable \"do_token\" {{}}\n\n\
                 provider \"digitalocean\" {{\n\
                 \x20 token = var.do_token\n\
                 }}\n\n\
                 resource \"digitalocean_droplet\" \"{hostname}\" {{\n\
                 \x20 name     = \"{hostname}\"\n\
                 \x20 size     = \"s-2vcpu-4gb\"\n\
                 \x20 image    = \"archlinux-x64\"\n\
                 \x20 region   = \"fra1\"\n\
                 \x20 user_data = file(\"cloud-init.yaml\")\n\
                 }}\n\n\
                 output \"ipv4\" {{\n\
                 \x20 value = digitalocean_droplet.{hostname}.ipv4_address\n\
                 }}\n"
            ),
            "export TF_VAR_do_token=<your DigitalOcean API token>",
        ),
        Provider::Aws => (
            format!(
                "terraform {{\n\
                 \x20 required_providers {{\n\
                 \x20   aws = {{ source = \"hashicorp/aws\" }}\n\
                 \x20 }}\n\
                 }}\n\n\
                 provider \"aws\" {{\n\
                 \x20 region = \"eu-central-1\"\n\
                 }}\n\n\
                 resource \"aws_instance\" \"{hostname}\" {{\n\
                 \x20 # Replace with a current Arch Linux AMI for your region —\n\
                 \x20 # AWS doesn't publish an official one, unlike Hetzner/DO.\n\
                 \x20 ami           = \"ami-REPLACE-ME\"\n\
                 \x20 instance_type = \"t3.medium\"\n\
                 \x20 user_data     = file(\"cloud-init.yaml\")\n\
                 \x20 tags = {{ Name = \"{hostname}\" }}\n\
                 }}\n\n\
                 output \"ipv4\" {{\n\
                 \x20 value = aws_instance.{hostname}.public_ip\n\
                 }}\n"
            ),
            "aws configure   # or AWS_ACCESS_KEY_ID / AWS_SECRET_ACCESS_KEY env vars",
        ),
    };

    std::fs::write(format!("{out}/main.tf"), &tf)?;

    let readme = format!(
        "# Monolith on {} — generated scaffolding\n\n\
         This is a starting point, not a finished deployment: review\n\
         `main.tf` and `cloud-init.yaml` before applying anything.\n\n\
         ```\n\
         cd {out}\n\
         {notes}\n\
         terraform init\n\
         terraform apply\n\
         ```\n\n\
         `cloud-init.yaml` hands off to Monolith's own bootstrap installer\n\
         once the instance boots — see scripts/bootstrap.sh in the repo.\n",
        provider_name(provider)
    );
    std::fs::write(format!("{out}/README.md"), &readme)?;

    println!(
        "{} Scaffolding written to {out}/ (main.tf, cloud-init.yaml, README.md)",
        "●".green()
    );
    println!("  {} Nothing was provisioned — review the files, then: cd {out} && terraform init && terraform apply", "→".blue());
    Ok(())
}

fn provider_name(p: &Provider) -> &'static str {
    match p {
        Provider::Hetzner => "Hetzner Cloud",
        Provider::Digitalocean => "DigitalOcean",
        Provider::Aws => "AWS",
    }
}
