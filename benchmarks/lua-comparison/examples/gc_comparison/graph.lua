local api = {}
function api.points(n)
    local values = {}
    local i = 0
    while i < n do
        values[i + 1] = {x = i, y = 1}
        i = i + 1
    end
    return values
end
function api.nodes(n)
    local values = {}
    local i = 0
    while i < n do
        local node = {value = i, edges = {}}
        node.edges[1] = node
        values[i + 1] = node
        i = i + 1
    end
    return values
end
function api.keep_points(values)
    local kept = {}
    local i = 1
    while i <= #values do
        kept[#kept + 1] = values[i]
        i = i + 64
    end
    return kept
end
api.keep_nodes = api.keep_points
function api.check_points(values, stride)
    local i = 1
    while i <= #values do
        if values[i].x ~= (i - 1) * stride or values[i].y ~= 1 then return -1 end
        i = i + 1
    end
    return #values
end
function api.check_nodes(values, stride)
    local i = 1
    while i <= #values do
        local node = values[i]
        if node.value ~= (i - 1) * stride or #node.edges ~= 1 then return -1 end
        node.value = node.value + 1
        if node.edges[1].value ~= node.value then return -1 end
        node.value = node.value - 1
        i = i + 1
    end
    return #values
end
return api
