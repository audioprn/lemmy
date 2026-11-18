-- audioprn: a fill term for a one-time license fee to monetise a fill (user, 2026-11-18).
ALTER TABLE post_script DROP CONSTRAINT post_script_fill_terms_check;

ALTER TABLE post_script ADD CONSTRAINT post_script_fill_terms_check CHECK (fill_terms IN ('ask_first', 'license_fee', 'paywall_ok', 'temporary_paywall', 'no_paywall'));
