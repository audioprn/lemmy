-- audioprn: up to 4 of a creator's own posts pinned on their page, under the pinned post.
-- Masterlists left the pinned post (the page has its own Masterlist button).
CREATE TABLE creator_page_pin (
    person_id int NOT NULL REFERENCES person ON UPDATE CASCADE ON DELETE CASCADE,
    post_id int NOT NULL REFERENCES post ON UPDATE CASCADE ON DELETE CASCADE,
    pinned_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (person_id, post_id)
);

ALTER TABLE creator_pin
    DROP COLUMN masterlists;

