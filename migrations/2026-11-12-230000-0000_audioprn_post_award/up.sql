-- audioprn: awards given to posts. One per person per post, never taken back. `pool` says which
-- allowance it came from: 'weekly' (posted or commented in the past 7 days) or 'monthly'
-- (a creator's 10 awards after a month with Content page posts).
CREATE TABLE post_award (
    post_id int NOT NULL REFERENCES post ON UPDATE CASCADE ON DELETE CASCADE,
    giver_id int NOT NULL REFERENCES person ON UPDATE CASCADE ON DELETE CASCADE,
    pool text NOT NULL CHECK (pool IN ('weekly', 'monthly')),
    published_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (post_id, giver_id)
);

CREATE INDEX idx_post_award_giver ON post_award (giver_id, published_at);
