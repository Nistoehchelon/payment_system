ALTER TABLE payments
    ALTER COLUMN amount TYPE BIGINT
    USING amount::bigint;