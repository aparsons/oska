# Hostile <script>alert("title")</script> notes

Every construct here tries to get markup or script past the converter.
None of it may survive as anything but text.

## Raw HTML

<script>alert("raw")</script>
<img src=x onerror=alert(1)>
<iframe src="javascript:alert(1)"></iframe>
<!-- a comment -->
<style>body{display:none}</style>

<div onclick="alert(1)">a block of raw HTML</div>

## Links that would run script

- [js lower](javascript:alert(1))
- [js upper](JavaScript:alert(1))
- [js tab](java	script:alert(1))
- [data](data:text/html;base64,PHNjcmlwdD5hbGVydCgxKTwvc2NyaXB0Pg==)
- [vbscript](vbscript:msgbox(1))
- [js entity](javascript&#58;alert(1))
- ![js in image](javascript:alert(1))

## Attribute injection

- [quote](https://example.com/" onmouseover="alert(1))
- [quote tight](https://example.com/"onmouseover="alert(1)")
- [angle](https://example.com/<script>)
- [backtick](https://example.com/`x`)
- ![x" onerror="alert(1)](pic.png)
- ![remote](https://evil.example/tracker.png)
- ![protocol relative](//evil.example/tracker.png)
- [single](https://example.com/'onmouseover='alert(1))

## Links that stay

- [ok](https://example.com/safe?a=1&b=2)
- [mail](mailto:someone@example.com)
- [glossary](../GLOSSARY.md#capability)
- [here](#raw-html)
- [unpublished](../crates/oskac/src/main.rs)

## Entities

&lt;script&gt; &#60;script&#62; &amp;

## Inline markup around markup

**<b>bold</b>** *<i>em</i>* `<code onclick="x">` and \<escaped\>.

[<img src=x onerror=alert(1)>](https://example.com/)

## A table

| <td onclick=x> | `a | b` |
|---|---|
| [x](javascript:alert(1)) | <script>cell</script> |

## A fence

```"><script>alert(1)</script>
<script>alert("in code")</script>
</code></pre><script>alert("break out")</script>
```

> <script>alert("quote")</script>

1. <script>alert("ordered")</script>
