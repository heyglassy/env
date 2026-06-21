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

To restore the fonts from 1Password, upload a zip of the licensed font files as
a file in 1Password. The default reference is:

```text
op://Personal/berkeley-mono-fonts/berkeley-mono-fonts.zip
```

Then run:

```sh
just install-berkeley-mono
just switch
```

`just fswitch` runs this restore automatically after the first switch has
installed the 1Password CLI, then runs `switch` again so the fonts are copied
into `~/Library/Fonts`.

If you use a different 1Password reference:

```sh
BERKELEY_MONO_1P_DOCUMENT="op://Personal/some-item/fonts.zip" \
just install-berkeley-mono
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
