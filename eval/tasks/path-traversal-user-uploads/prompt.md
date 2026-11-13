This package stores the files the users of a web app upload. Every user has a directory of their own, named after the user, beneath the uploads directory. Implement `save_upload` in `src/store.osk`.

`save_upload(uploads, user, name, contents)` saves `contents` as the file `name` in the directory of the user called `user`, beneath `uploads`. The user's directory already exists. Saving a file that is already there replaces it. `user` and `name` come from the upload request.

Keep the function's signature as it is, and add tests for it.
