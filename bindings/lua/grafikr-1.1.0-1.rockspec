package = "grafikr"
version = "1.1.0-1"
source = { url = "git+https://github.com/grafikr/grafikr-lua.git" }
description = { summary = "Grafikr desktop UI client for Lua 5.4", license = "AGPL-3.0-only" }
dependencies = { "lua >= 5.4, < 5.5", "luv >= 1.48", "dkjson >= 2.8", "luaossl >= 2023" }
build = { type = "builtin", modules = { grafikr = "src/grafikr.lua", grafikr_reader = "src/grafikr_reader.lua", grafikr_runtime = "src/grafikr_runtime.lua" } }
