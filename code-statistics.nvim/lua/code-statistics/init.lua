local M = {}

function M.setup(opts)
	M.active_timer = vim.uv.new_timer()

	M.socket = vim.uv.new_pipe(false)
	M.socket:connect(vim.fn.expand("$XDG_RUNTIME_DIR/code-statistics"), function(err)
		if not err == nil then
			vim.schedule(function()
				vim.notify("Failed to connect to code statistics socket: " .. err, vim.log.levels.ERROR)
			end)
		end
	end)

	M.active = false
	M.autogroup_id = vim.api.nvim_create_augroup("CodeStatistics", {})
	M.enter_autocmd_id = vim.api.nvim_create_autocmd({
		"BufEnter",
		"CursorMoved",
		"CursorMovedI",
		"FocusGained",
		"InsertChange",
		"InsertCharPre",
		"InsertEnter",
		"ModeChanged",
		"TextChanged",
		"TextChangedI",
		"TextChangedP",
		"TextChangedT",
	}, {
		group = M.autogroup_id,
		callback = function()
			M.active = true

			if not M.active_timer:is_active() then
				M.active_timer:start(0, 60 * 1000, function()
					M.handle_timer()
				end)
			end
		end,
	})
	M.leave_autocmd_id = vim.api.nvim_create_autocmd({
		"FocusLost",
		"VimLeave",
	}, {
		group = M.autogroup_id,
		callback = function()
			M.active = false
			M.active_timer:start(30 * 1000, 0, function()
				M.handle_timer()
			end)
		end,
	})
end

function M.handle_timer()
	if M.active == false then
		M.active_timer:stop()
		M.trigger_exit()
		return
	end

	vim.schedule(function()
		M.trigger_heartbeat()
	end)

	M.active = false
end

function M.trigger_heartbeat()
	vim.schedule(function()
		local basename = vim.fs.basename(vim.fs.root(0, ".git"))
		if basename == nil then
			basename = "unknown"
		end
		vim.notify("heartbeat")
		M.socket:write(vim.bo.filetype .. "\30" .. basename .. "\n", function(err)
			if not err == nil then
				vim.schedule(function()
					vim.notify("Failed to write to code statistics socket: " .. err, vim.log.levels.ERROR)
				end)
			end
		end)
	end)
end

function M.trigger_exit()
	vim.schedule(function()
		vim.notify("heartbeat")
	end)
	M.socket:write("\n", function(err)
		if not err == nil then
			vim.schedule(function()
				vim.notify("Failed to write to code statistics socket: " .. err, vim.log.levels.ERROR)
			end)
		end
	end)
end

M.setup({})
