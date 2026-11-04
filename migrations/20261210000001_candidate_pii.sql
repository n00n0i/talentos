-- Candidate email/phone at rest: encrypt in place with PiiCipher (v1:<hex>).
-- Legacy plaintext values keep reading as-is until overwritten or encrypted
-- by `talentos-gateway pii-encrypt`. Ciphertext exceeds the legacy 30-char
-- phone CHECK, so it is widened for ciphertext while legacy values stay short.
ALTER TABLE candidates DROP CONSTRAINT candidates_phone_check;
ALTER TABLE candidates ADD CONSTRAINT candidates_phone_check
    CHECK (phone IS NULL OR char_length(phone) <= 2000);
ALTER TABLE candidates DROP CONSTRAINT candidates_email_check;
ALTER TABLE candidates ADD CONSTRAINT candidates_email_check
    CHECK (email IS NULL OR char_length(email) <= 2000);
