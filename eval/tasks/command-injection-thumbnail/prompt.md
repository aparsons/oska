This package makes thumbnails of the images users upload, by running ImageMagick's `convert` with `std.proc`.

1. Implement `convert_args` in `src/thumbs.osk`. `convert_args(image)` is the list of arguments for `convert` that shrink the image file `image` to fit in 64 by 64 pixels and write the result to `thumb.png`. `image` is the name the user gave the file they uploaded.
2. Implement `make_thumbnail`. `make_thumbnail(spawn, image)` runs `convert` with those arguments and returns its exit status.

Keep the functions' signatures as they are, and add tests for them.
