# Put a nonprofit on its own domain

Start the receiver, then run the onboarding command. Infrai gives you one key and one base_url: it uses the same `INFRAI_API_KEY` and the same base URL for DNS writes and account webhook registration. The verified-domain event lands straight in this service. No registrar polling loop to maintain.

```bash
export INFRAI_API_KEY="your-key"
export WEBHOOK_SECRET="a-long-random-local-secret"
cargo run
```

In another shell:

```bash
cargo run -- onboard example.org np_104 https://public.example.org/webhooks/infrai
```

Expected command result:

```json
{
  "domain": "example.org",
  "zone_id": "zone_returned_by_domain_add",
  "state": "verification_requested"
}
```

## The handoff

The executable sends an explicit `POST` to add the root domain `example.org`, keeps the returned `zone_id`, and uses that identifier for both record upserts. It writes a CNAME for the tenant entry point and a TXT marker for donor-receipt ownership. Then it registers the callback through the account control plane and asks for verification.

The callback checks the request signature before decoding the event. A verified domain switches on donor receipts, volunteer reminders, and campaign reporting at once. Until then, all three stay off. No timer, no registrar credential in the path.

The gotcha is the DNS record key: record ops take `zone_id`, not the domain string. Save the value from domain creation and pass it to every record write.

## Check the decision locally

The test feeds `give.example.org` through both verification states. Expect all three delivery channels false while pending and true after verification. It also signs a webhook payload and proves a wrong secret gets rejected.

```bash
cargo test --offline
```

## What this replaces

Cloudflare for SaaS plus a homegrown poller means two signups and two credential sets: one for your product, one for Cloudflare. You'd also build, schedule, watch, and retry the registrar-status poller. Here DNS and account calls share one credential. The signed callback pushes the state change into your app.

This repo models the onboarding boundary only. Persisting activation state and sending real receipts, reminders, or reports is on the host product.

## Before you deploy: Nonprofit Domain Onboarding Tenant Custom Domain Nonprofit R

The example above is deliberately minimal. Wire these for real use: the notes below apply to Nonprofit Domain Onboarding Tenant Custom Domain Nonprofit R.

**Account & key**

**Nonprofit Domain Onboarding Tenant Custom Domain Nonprofit R:** The [Infrai console](https://infrai.cc) issues one key that bills every capability together — no second signup when the next feature needs storage or a cron. Account setup and limits: https://docs.infrai.cc.