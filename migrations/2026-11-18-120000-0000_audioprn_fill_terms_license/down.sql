UPDATE post_script SET fill_terms = 'ask_first' WHERE fill_terms = 'license_fee';

ALTER TABLE post_script DROP CONSTRAINT post_script_fill_terms_check;

ALTER TABLE post_script ADD CONSTRAINT post_script_fill_terms_check CHECK (fill_terms IN ('ask_first', 'paywall_ok', 'temporary_paywall', 'no_paywall'));
