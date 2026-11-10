ALTER TABLE invoices
    ALTER COLUMN amount TYPE BIGINT
    USING amount::bigint;