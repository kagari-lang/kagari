local function mix(value)
    return (value * 17 + 13) % 65521
end

local function run(n)
    local i = 0
    local sum = 0
    while i < n do
        sum = sum + mix(i % 97)
        i = i + 1
    end
    return sum
end

return function()
    return run(__N__)
end
