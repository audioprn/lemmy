-- audioprn: `w:` search (writer credit, without "u/") by index.
CREATE INDEX idx_post_credit_writer ON post_credit (lower(regexp_replace(name, '^u/', '', 'i')))
WHERE
    role = 'writer';

