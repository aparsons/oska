// The search box of https://oskalang.dev. `site/build.osk` copies this file
// to `_site/search.js`, and every page loads it last.
//
// The site works without it: it only adds a box to the header that looks
// through `/search.json`, the index the generator writes (each page's URL,
// title, headings, and the start of its text). It reads nothing else, and
// writes to the page only with `textContent` and `href`, never markup,
// which the page's CSP (`script-src 'self'`, `connect-src 'self'`) holds it
// to as well.

(function () {
  "use strict";

  var LIMIT = 10;

  // The lower-case words in `text`.
  function words(text) {
    return String(text)
      .toLowerCase()
      .split(/[^\p{L}\p{N}_]+/u)
      .filter(Boolean);
  }

  // The pages in `index` that have every word of `query` somewhere in them,
  // best first: a word in the title counts most, then one in a heading,
  // then one in the text. At most LIMIT.
  function search(index, query) {
    var terms = words(query);
    if (terms.length === 0) {
      return [];
    }
    var results = [];
    index.forEach(function (page) {
      var title = String(page.title).toLowerCase();
      var headings = page.headings.join(" ").toLowerCase();
      var text = String(page.text).toLowerCase();
      var score = 0;
      for (var i = 0; i < terms.length; i++) {
        var here =
          (title.indexOf(terms[i]) >= 0 ? 10 : 0) +
          (headings.indexOf(terms[i]) >= 0 ? 4 : 0) +
          (text.indexOf(terms[i]) >= 0 ? 1 : 0);
        if (here === 0) {
          return;
        }
        score += here;
      }
      results.push({ page: page, score: score });
    });
    results.sort(function (a, b) {
      return b.score - a.score || a.page.title.localeCompare(b.page.title);
    });
    return results.slice(0, LIMIT).map(function (r) {
      return r.page;
    });
  }

  if (typeof module !== "undefined" && module.exports) {
    module.exports = { search: search };
  }
  if (typeof document === "undefined") {
    return;
  }

  var header = document.querySelector("header");
  if (!header || typeof fetch !== "function") {
    return;
  }

  var box = document.createElement("div");
  box.className = "search";
  var input = document.createElement("input");
  input.type = "search";
  input.placeholder = "Search the docs";
  input.setAttribute("aria-label", "Search the docs");
  var list = document.createElement("ul");
  list.className = "search-results";
  list.hidden = true;
  box.appendChild(input);
  box.appendChild(list);
  header.appendChild(box);

  var loading = null;

  // The index, fetched the first time it is needed.
  function load() {
    if (!loading) {
      loading = fetch("/search.json").then(function (response) {
        if (!response.ok) {
          throw new Error("search.json: " + response.status);
        }
        return response.json();
      });
    }
    return loading;
  }

  function say(message) {
    var item = document.createElement("li");
    item.textContent = message;
    list.appendChild(item);
  }

  function show() {
    var query = input.value;
    list.textContent = "";
    list.hidden = query.trim() === "";
    if (list.hidden) {
      return;
    }
    load().then(
      function (index) {
        if (input.value !== query) {
          return;
        }
        var found = search(index, query);
        if (found.length === 0) {
          say("No pages match.");
        }
        found.forEach(function (page) {
          var item = document.createElement("li");
          var link = document.createElement("a");
          link.href = page.url;
          link.textContent = page.title;
          item.appendChild(link);
          list.appendChild(item);
        });
      },
      function () {
        if (input.value === query) {
          say("Search is unavailable.");
        }
      }
    );
  }

  input.addEventListener("input", show);
  input.addEventListener("focus", function () {
    load().catch(function () {});
  });
  input.addEventListener("keydown", function (event) {
    if (event.key === "Escape") {
      input.value = "";
      show();
    }
  });
})();
