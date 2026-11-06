-- No metatables, callbacks or allocations in the measured loop bodies.
-- Local controls retain one initial/final table access per whole batch.
local api = {}

function api.named_read(object, count)
    local sum = 0
    for i = 1, count do
        sum = sum + object.value
    end
    return sum
end

function api.named_write(object, count)
    for i = 1, count do
        object.value = i - 1
    end
    return object.value
end

function api.named_increment(object, count)
    for i = 1, count do
        object.value = object.value + 1
    end
    return object.value
end

function api.indexed_read(object, count)
    local sum = 0
    for i = 1, count do
        sum = sum + object[1]
    end
    return sum
end

function api.indexed_write(object, count)
    for i = 1, count do
        object[1] = i - 1
    end
    return object[1]
end

function api.indexed_increment(object, count)
    for i = 1, count do
        object[1] = object[1] + 1
    end
    return object[1]
end

function api.local_read(object, count)
    local value = object.value
    local sum = 0
    for i = 1, count do
        sum = sum + value
    end
    return sum
end

function api.local_write(object, count)
    local value = object.value
    for i = 1, count do
        value = i - 1
    end
    object.value = value
    return value
end

function api.local_increment(object, count)
    local value = object.value
    for i = 1, count do
        value = value + 1
    end
    object.value = value
    return value
end

return api
