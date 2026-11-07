CREATE TABLE callback_headers (
    id      bigserial PRIMARY KEY,
    payment_id text NOT NULL,
    key        text NOT NULL,
    value      text NOT NULL
);

CREATE TABLE litecoin_address_indexes (
    primary_key bigserial PRIMARY KEY,
    payment_id  text   NOT NULL,
    index       bigint NOT NULL
);

CREATE TABLE monero_integrated_payment_ids (
    primary_key       bigserial PRIMARY KEY,
    monero_payment_id text NOT NULL,
    payment_id        text NOT NULL
);

CREATE TABLE payments (
    primary_key         bigserial PRIMARY KEY,
    id                  text        NOT NULL DEFAULT gen_random_uuid()::text,
    status_update_callback text     NOT NULL,
    success_callback    text        NOT NULL,
    method              text        NOT NULL,
    amount              text        NOT NULL,
    address             text        NOT NULL,
    created_at          timestamptz NOT NULL DEFAULT now(),
    completed_at        timestamptz,
    sweeped_at          timestamptz
);

CREATE TABLE tokens (
    id    bigserial PRIMARY KEY,
    token text NOT NULL
);