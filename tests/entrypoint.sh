#!/bin/sh
set -e

echo "Loading test fixtures into VTN database..."
PGPASSWORD=openadr psql -h test-db -U openadr -d openadr \
  -f /fixtures/test_user_credentials.sql
echo "Fixtures loaded."

echo "Provisioning ven-2 via API..."
python provision_ven2.py
echo "Provisioning done."

exec behave "$@"
