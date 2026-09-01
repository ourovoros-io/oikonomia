# Paddle fulfillment runbook

Ops guide for manual license fulfillment via Paddle. Not for the public.

## Merchant of record

Oikonomia is sold via Paddle under the Ourovoros.io account. Paddle invoices the buyer, handles EU VAT recovery, and issues refunds. Ourovoros (the operator) is the contract counterparty for chargebacks and disputes.

Checkout: https://ourovoros.io/oikonomia

## Product and expiry convention

Product name in Paddle: "Oikonomia license"

Expiry default: issue date + 1 year. This date is written into the `.lic` file and displayed in Settings → License.

**GO-LIVE decision (record here):** Licenses issued before [DATE] follow the 1-year default. After that date, the convention is [DECISION: 1-year | perpetual | other].

## Fulfillment workflow (v1 — manual)

1. Buyer completes checkout on Paddle and receives an order confirmation email. Paddle also sends the operator an order notification.

2. On the operator machine (air-gapped or at least offline key management), run:
   ```
   cargo run -p oikonomia-mint -- issue --key <path-to-offline-key> --email <buyer@domain> --expiry YYYY-MM-DD --out license.lic
   ```
   Replace `<offline-key>` with the path to the offline Ed25519 secret key (see "Key ceremony" in `release.md`). The `--expiry` date is the issue date plus the agreed term (default: issue date + 1 year).

3. Send a reply email to the buyer using the template below.

4. Archive the order ID and expiry date in your records (a spreadsheet or database; never commit buyer PII to the repository).

## Email template

```
Subject: Your Oikonomia license is ready

Hi [buyer name],

Thank you for purchasing Oikonomia. Your license file is attached as license.lic.

To import it:
1. Open Oikonomia
2. Go to Settings → License → Import license
3. Select license.lic

Your license expires on YYYY-MM-DD. Read the full license agreement at https://github.com/ourovoros-io/oikonomia/blob/main/EULA.md.

Refunds: Contact Paddle (refunds handled by Paddle, not here).

Best,
[Operator name]
```

## Constraints

- The license **private key never leaves the operator's machine**. It does not go to GitHub, CI, Paddle servers, the app, or any internet-connected service.
- Webhook automation (Paddle → auto-mint) is out of scope for v1. Fulfillment is manual.
- Buyer PII (names, email addresses, order details) must never be committed to the repository.
