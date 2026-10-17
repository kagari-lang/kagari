local function run(n)
    local values = {}
    local i = 0
    while i < n do
        values[i * 17 + 1] = i % 97
        i = i + 1
    end
    i = 0
    while i < n do
        local key = i * 17 + 1
        local value = values[key]
        if value == nil then value = -1 end
        values[key] = value + 1
        i = i + 1
    end
    i = 0
    local sum = 0
    while i < n do
        local value = values[i * 17 + 1]
        if value == nil then value = -1 end
        sum = sum + value
        i = i + 1
    end
    return sum
end

return function()
    return run(__N__)
end
