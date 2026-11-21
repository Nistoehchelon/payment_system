-- Add migration script here
ALTER TABLE invoices
  ALTER COLUMN amount TYPE TEXT
  USING amount::text;