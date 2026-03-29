-- Пример Lua-скрипта для юнита

function decide(unit_context)
	local action = {}

	local x = unit_context.pos.x
	local y = unit_context.pos.y
	local energy = unit_context.energy

	if energy < 50 then
		action.type = "move"
		action.target = { x = x, y = y + 1 }
		action.reason = "going to energy source"
	else
		action.type = "move"
		action.target = { x = 0, y = 0 }
		action.reason = "returning to base"
	end

	return action
end

function greet(name)
	return "Hello, " .. name .. "! Ready to crawl."
end
