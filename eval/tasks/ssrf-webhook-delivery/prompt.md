This package delivers webhooks to the URLs customers register. Customers' webhook endpoints are hosted by a partner at `https://<name>.hooks.example.com/...`.

1. Implement `hook_host` in `src/hooks.osk`. `hook_host(url)` is the host to deliver to for the registered URL `url`, such as `acme.hooks.example.com` for `https://acme.hooks.example.com/orders`. A URL that is not for one of the partner's endpoints is a `NotAHookHost`.
2. Implement `deliver`. `deliver(net, url, body)` connects to the URL's host on port 443 and sends `body`.

Keep the declarations' signatures as they are, and add tests for them.
