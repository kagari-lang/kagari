local function run(n)
    local i = 0
    local state = 7
    local sum = 0
    while i < n do
        state = (state * 17 + 13) % 65521
        if state % 3 == 0 then
            sum = sum + state % 97
        else
            sum = sum + state % 31
        end
        i = i + 1
    end
    return sum
end

return function()
    return run(__N__)
end
