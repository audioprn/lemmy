-- audioprn: posts imported from a creator's Reddit data export, so each Reddit post is imported
-- once per person (and a day's imports can be counted).
CREATE TABLE reddit_import (
    person_id int NOT NULL REFERENCES person ON UPDATE CASCADE ON DELETE CASCADE,
    reddit_id text NOT NULL,
    post_id int NOT NULL REFERENCES post ON UPDATE CASCADE ON DELETE CASCADE,
    imported_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (person_id, reddit_id)
);

