BEGIN;

-- (A) 20261101000003 + 20261214000001: 96-byte ASCII-hex payload -> real 48-byte digest.
--     convert_from(...,'SQL_ASCII') turns the stored bytes back into the hex text, decode() un-hexes it.
--     Description is left untouched (not a safety gate).
UPDATE _sqlx_migrations
   SET checksum = decode(convert_from(checksum, 'SQL_ASCII'), 'hex')
 WHERE version IN (20261101000003, 20261214000001)
   AND octet_length(checksum) = 96;

-- (B) 20261215000001: restate to the digest of the file as it exists today.
--     The live schema already equals a fresh build.
UPDATE _sqlx_migrations
   SET checksum = decode('334a50c77b373d34b1a26e26fc9433194348c27b5a734359d761f70e7d77ff4f5352d7c77436559004cd273b47a44c3e', 'hex')
 WHERE version = 20261215000001;

-- (C) 20261114000001: objects are present; record what was applied.
INSERT INTO _sqlx_migrations
       (version, description, success, checksum, execution_time, installed_on)
VALUES (20261114000001, 'gl accounts cost center', true,
        decode('821d71a8e21d94d6ca92d1adf1880a167bb583bb49c896af5d662ffc7edcc96e8eb384cb3cf54a9a94d4252ae1bbd38a','hex'),
        0, now())
ON CONFLICT (version) DO NOTHING;

COMMIT;
