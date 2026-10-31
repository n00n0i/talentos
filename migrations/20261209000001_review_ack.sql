-- Employee acknowledgement of a submitted performance review.
ALTER TABLE performance_reviews ADD COLUMN acknowledged_at TIMESTAMPTZ;

ALTER TABLE performance_reviews DROP CONSTRAINT performance_reviews_status_check;
ALTER TABLE performance_reviews ADD CONSTRAINT performance_reviews_status_check
    CHECK (status IN ('pending','submitted','acknowledged'));
