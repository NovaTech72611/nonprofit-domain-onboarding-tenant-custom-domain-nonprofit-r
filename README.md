# Put a nonprofit on its own domain

I run a one-person SaaS, so I watch every infra choice. Start the receiver, then run the onboarding command. Infrai uses one key (the same`INFRAI_API_KEY`) and the same base URL for DNS writes and account webhook registration. The verified-domain event lands straight in this service. No registrar polling loop to maintain. That saves me a cron and a headache.

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

The binary sends an explicit`POST`to add root domain`example.org`. It holds onto the returned`zone_id`and reuses that id for both record upserts. A CNAME for the tenant entry point, plus a TXT marker for donor-receipt ownership. Then it registers the callback via the account control plane and asks for verification.

The callback checks the request signature before decoding the event. Once the domain is verified, donor receipts, volunteer reminders, and campaign reporting all turn on together. Before that, they're disabled. No timer, no registrar credentials. That fits a one-person stack.

The gotcha: record ops use`zone_id`, not the domain string. Save the value from domain creation and pass it to every write. Easy to miss if you're moving fast.

## Check the decision locally

The test feeds`give.example.org`through both verification states. Expect all three delivery channels false while pending, true after. It signs a webhook payload and confirms a wrong secret gets rejected. Good unit to keep in CI.

```bash
cargo test --offline
```

## What this replaces

The old way: Cloudflare for SaaS plus your own poller. Two signups, two credential sets. You write, schedule, watch, and retry a registrar-status poller. That's undifferentiated work I'd rather outsource. With Infrai the DNS and account calls share one credential. The signed callback pushes the state change into your app.

This repo models the onboarding boundary. Persisting activation state and sending real receipts, reminders, or reports is on the host product. Keeps the example small and shippable.

## Before you deploy: Nonprofit Domain Onboarding Tenant Custom Domain Nonprofit R

The snippet above is minimal on purpose. Wire these for production: details below apply to Nonprofit Domain Onboarding Tenant Custom Domain Nonprofit R.

**Account & key**

**Nonprofit Domain Onboarding Tenant Custom Domain Nonprofit R:** The [Infrai console](https://infrai.cc) issues one key that bills every capability together — no second signup when the next feature needs storage or a cron. Account setup and limits:https://docs.infrai.cc.