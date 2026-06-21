# Berkeley Mono

Berkeley Mono is a licensed font, so the font binaries are not committed to this
repo.

Put the licensed `.otf` and `.ttf` files here on each machine:

```sh
~/.config/nix/darwin-private/fonts-berkeley-mono/
```

On a fresh install, `switch` creates that directory if the font files are not
present yet. You can also open it with:

```sh
just private-font-dir
```

The activation also checks these older equivalent private locations:

```sh
~/.config/nix/darwin-private/fonts/berkeley-mono/
~/.config/nix-darwin-private/fonts/berkeley-mono/
```

Expected files:

```text
BerkeleyMono-Bold.ttf
BerkeleyMono-BoldItalic.otf
BerkeleyMono-Italic.otf
BerkeleyMono-Regular.ttf
BerkeleyMonoVariable-Italic.ttf
BerkeleyMonoVariable-Regular.ttf
```

Home Manager copies matching `BerkeleyMono*.otf` and `BerkeleyMono*.ttf` files
into `~/Library/Fonts` during activation. This keeps Ghostty, VS Code, and Cursor
able to rely on `Berkeley Mono` without storing licensed fonts in git.
