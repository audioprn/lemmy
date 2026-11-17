-- audioprn: a creator can turn off "Share with Creator" script recommendations.
ALTER TABLE creator_application
    ADD COLUMN recommendations boolean NOT NULL DEFAULT TRUE;

