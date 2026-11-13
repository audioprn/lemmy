-- audioprn: the Content feed is sorted by New by default (spec).
ALTER TABLE local_site
    ALTER COLUMN default_post_sort_type SET DEFAULT 'New';

ALTER TABLE local_user
    ALTER COLUMN default_post_sort_type SET DEFAULT 'New';

UPDATE
    local_site
SET
    default_post_sort_type = 'New';

UPDATE
    local_user
SET
    default_post_sort_type = 'New';
