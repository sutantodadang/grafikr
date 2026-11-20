local timers = 0
package.preload["luv"] = function()
  return { new_timer=function()
    timers=timers+1
    local timer={}
    function timer:start(_,_,callback) self.callback=callback end
    function timer:stop() self.stopped=true end
    function timer:is_closing() return self.closing==true end
    function timer:close() self.closing=true end
    return timer
  end }
end
package.preload["dkjson"] = function() return {} end
local grafikr=require("grafikr")
local calls={}
local app=setmetatable({},grafikr.App)
function app:on(node,event,callback,on_subscribed)
  assert(node=="7" and event=="data.request")
  callback({payload={revision="2",request="9",start=4,count=2}})
  local subscription={close=function(_,done) calls[#calls+1]="event.unsubscribe"; if done then done(nil) end end}
  on_subscribed(subscription,nil)
end
function app:request(op,args,callback)
  calls[#calls+1]=op
  if callback then callback({ok=true,result={revision="2"}}) end
  return tostring(#calls)
end

local provider,create_error
app:create_collection_provider({handle="7"},{row_count=20,row_height=24,columns={"label"}},function(value,error) provider=value; create_error=error end)
assert(provider and not create_error)
local first
provider:next_request(0,function(request,error) assert(not error); first=request end)
assert(first and first.revision=="2" and first.start==4 and first.count==2)
provider:provide(first,{{"a"}},function(reply) assert(reply.ok) end)
provider:update({row_count=1,row_height=24,columns={"label"}},function(reply) assert(reply.ok) end)

local overflow={payload={revision="2",request="10",start=6,count=1}}
provider:_receive(overflow); provider:_receive(overflow)
local queued
provider:next_request(0,function(request) queued=request end)
assert(queued and queued.request=="10")
local queue_error
provider:next_request(0,function(_,error) queue_error=error end)
assert(queue_error and queue_error.code=="queue_full")

local woke=false
provider:next_request(1000,function(request) assert(request==nil); woke=true end)
assert(timers==1)
provider:close()
assert(woke and provider.disposed and calls[#calls]=="event.unsubscribe")
local invalid,invalid_error=app:create_collection_provider({handle="7"},{row_count=-1,row_height=24,columns={"label"}},function() end)
assert(invalid==nil and invalid_error.code=="invalid_value")
print("Grafikr Lua collection provider unit checks passed")
