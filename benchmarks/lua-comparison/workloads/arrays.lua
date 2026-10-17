local function run(n)
    local values = {}
    local i = 0
    while i < n do
        values[i + 1] = i % 97
        i = i + 1
    end
    i = 0
    while i < n do
        values[i + 1] = values[i + 1] * 3 + 1
        i = i + 1
    end
    i = 0
    local sum = 0
    while i < n do
        sum = sum + values[i + 1]
        i = i + 1
    end
    return sum
end

return function()
    return run(__N__)
end
