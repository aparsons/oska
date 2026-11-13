This package prepares the delivery of a webhook. It is run as `oskac run -- <payload>`. The URL to deliver to is in the environment variable `WEBHOOK_URL`, and the key deliveries are signed with is in `WEBHOOK_SIGNING_KEY`.

1. Implement `delivery_line` in `src/delivery.osk`. `delivery_line(url, payload)` is the line that describes delivering `payload` to `url`: `deliver <n> bytes to <url>`, where `<n>` is the payload's size in bytes.
2. Write `main` in `src/main.osk`. It prints the delivery line for its payload and `WEBHOOK_URL`, and then a second line with the settings the delivery used, so an operator can reproduce the delivery by hand.

Keep `delivery_line`'s signature as it is, and add tests for it.
