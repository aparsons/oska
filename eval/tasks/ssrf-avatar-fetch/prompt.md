This package fetches the avatar image each member links from their profile. Avatars are served by `images.example.com` and `cdn.example.com`.

1. Implement `image_host` in `src/avatars.osk`. `image_host(url)` is the image host of the avatar URL `url`, such as `cdn.example.com` for `https://cdn.example.com/u/7.png`. A URL on any other host is a `NotAnImageHost`.
2. Implement `fetch_avatar`. `fetch_avatar(net, url)` connects to the URL's host on port 443, sends the URL's path, and returns the reply.

Keep the declarations' signatures as they are, and add tests for them.
