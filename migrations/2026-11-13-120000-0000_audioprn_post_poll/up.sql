-- audioprn: polls on creators' page posts. One choice per vote, votes can't be changed, results
-- show after voting (or once the poll ends), and every poll has an end date.
CREATE TABLE post_poll (
    post_id int PRIMARY KEY REFERENCES post ON UPDATE CASCADE ON DELETE CASCADE,
    question text NOT NULL CHECK (char_length(question) BETWEEN 1 AND 200),
    options text[] NOT NULL CHECK (cardinality(options) BETWEEN 2 AND 6),
    ends_at timestamptz NOT NULL
);

CREATE TABLE post_poll_vote (
    post_id int NOT NULL REFERENCES post_poll ON UPDATE CASCADE ON DELETE CASCADE,
    person_id int NOT NULL REFERENCES person ON UPDATE CASCADE ON DELETE CASCADE,
    choice int NOT NULL,
    published_at timestamptz NOT NULL DEFAULT now(),
    PRIMARY KEY (post_id, person_id)
);
