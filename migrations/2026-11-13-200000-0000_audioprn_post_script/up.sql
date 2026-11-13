-- audioprn: scripts on Script Offer posts, readable here as well as on scriptbin.works or another
-- site. The text is for logged-in readers only, with a per-account reading limit.
CREATE TABLE post_script (
    post_id int PRIMARY KEY REFERENCES post ON UPDATE CASCADE ON DELETE CASCADE,
    body text NOT NULL DEFAULT '' CHECK (char_length(body) <= 200000),
    link text,
    fill_terms text CHECK (fill_terms IN ('ask_first', 'paywall_ok', 'temporary_paywall', 'no_paywall')),
    updated_at timestamptz NOT NULL DEFAULT now()
);

-- Who read which script when, for the reading limit.
CREATE TABLE post_script_read (
    person_id int NOT NULL REFERENCES person ON UPDATE CASCADE ON DELETE CASCADE,
    post_id int NOT NULL REFERENCES post ON UPDATE CASCADE ON DELETE CASCADE,
    read_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (person_id, post_id)
);
