-- audioprn: applications to become a verified creator.
-- A person is a verified creator when their application is approved.
CREATE TABLE creator_application (
    id serial PRIMARY KEY,
    person_id int NOT NULL UNIQUE REFERENCES person ON UPDATE CASCADE ON DELETE CASCADE,
    -- Exactly 3 links to the creator's audios or scripts on other sites.
    content_links text[] NOT NULL,
    -- Optional links to the creator's social media, as many as needed (capped at 20).
    social_links text[] NOT NULL DEFAULT '{}',
    -- NULL = pending, true = approved, false = denied.
    approved boolean,
    admin_id int REFERENCES person ON UPDATE CASCADE ON DELETE SET NULL,
    deny_reason text,
    published_at timestamptz NOT NULL DEFAULT now(),
    updated_at timestamptz,
    CONSTRAINT creator_application_three_content_links CHECK (cardinality(content_links) = 3),
    CONSTRAINT creator_application_max_social_links CHECK (cardinality(social_links) <= 20)
);

CREATE INDEX idx_creator_application_pending ON creator_application (published_at)
WHERE
    approved IS NULL;
