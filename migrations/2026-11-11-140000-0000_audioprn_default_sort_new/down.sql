ALTER TABLE local_site
    ALTER COLUMN default_post_sort_type SET DEFAULT 'Active';

ALTER TABLE local_user
    ALTER COLUMN default_post_sort_type SET DEFAULT 'Active';
