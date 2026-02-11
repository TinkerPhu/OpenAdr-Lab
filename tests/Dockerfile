FROM python:3.12-slim

RUN apt-get update && apt-get install -y --no-install-recommends \
    postgresql-client curl docker.io \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /tests

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt \
    && playwright install chromium --with-deps

COPY behave.ini .
COPY features/ features/
COPY provision_ven2.py .
COPY entrypoint.sh .
RUN chmod +x entrypoint.sh

ENTRYPOINT ["./entrypoint.sh"]
