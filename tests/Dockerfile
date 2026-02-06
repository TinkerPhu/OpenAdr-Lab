FROM python:3.12-alpine

RUN apk add --no-cache postgresql-client

WORKDIR /tests

COPY requirements.txt .
RUN pip install --no-cache-dir -r requirements.txt

COPY behave.ini .
COPY features/ features/
COPY entrypoint.sh .
RUN chmod +x entrypoint.sh

ENTRYPOINT ["./entrypoint.sh"]
