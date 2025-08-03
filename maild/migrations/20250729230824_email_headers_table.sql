-- Add migration script here
CREATE TABLE email_headers (
    email_id UUID NOT NULL REFERENCES emails(id) ON DELETE CASCADE,
    key TEXT NOT NULL,
    value TEXT NOT NULL
);
-- Optionally, you can add an index for faster lookups by email_id
CREATE INDEX idx_email_headers_email_id ON email_headers(email_id);

-- Migrate headers from emails.headers JSONB to email_headers table
INSERT INTO email_headers (email_id, key, value)
SELECT
    id AS email_id,
    header_pair->>0 AS key,
    header_pair->>1 AS value
FROM
    emails,
    LATERAL jsonb_array_elements(headers) AS header_pair
WHERE
    jsonb_typeof(headers) = 'array'
    AND jsonb_array_length(headers) > 0
    AND jsonb_typeof(header_pair) = 'array'
    AND jsonb_array_length(header_pair) = 2
    AND jsonb_typeof(header_pair->0) = 'string'
    AND jsonb_typeof(header_pair->1) = 'string';

-- Drop the old headers column
ALTER TABLE emails DROP COLUMN headers;
