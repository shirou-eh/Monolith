FROM python:3.12-alpine
RUN addgroup -S bot && adduser -S bot -G bot
WORKDIR /app
COPY requirements.txt .
RUN pip install --no-cache-dir --no-compile -r requirements.txt
COPY --chown=bot:bot . .
USER bot
ENV PYTHONUNBUFFERED=1 PYTHONDONTWRITEBYTECODE=1
CMD ["python", "-u", "src/main.py"]
