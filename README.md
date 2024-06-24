# mdwrap

A small Rust program that wraps markdown files. Uses the
[comrak](https://docs.rs/comrak/) AST library so that rendering
remains accurate.

## Usage

```
Wrap markdown files

Usage: wrapsody-md [OPTIONS]

Options:
  -f, --file <FILE>            Input file, use stdio if none provided
  -o, --outfile <OUTFILE>      Output file, use stdout if none provided
  -l, --linewidth <LINEWIDTH>  Maximum line width in chars [default: 80]
  -h, --help                   Print help
  -V, --version                Print version
```

Write wrapped input.md to output.md.


``` bash
mdwrap -f input.md -o output.md
```

Write wrapped markdown from stdin to stdout

``` bash
mdwrap
```

### Use with (neo)vim

As a vim command

``` vimscript
%!mdwrap
```

With [formatter.nvim](https://github.com/mhartington/formatter.nvim)
and lazy.nvim (plugin manager).

``` lua
local function wrapper()
	return {
		exe = "mdwrap",
		args = { "-l", "70" }, -- or whatever line width you want
		stdin = true,
	}
end

return {
	"mhartington/formatter.nvim",
	config = function(plugin)
		local formatter_setup = {
			logging = false,
			filetype = {
				markdown = { wrapper },
			},
		}
		require("formatter").setup(formatter_setup)
	end,
	cmd = "FormatWrite",
 }
```
