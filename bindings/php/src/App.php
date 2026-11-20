<?php
declare(strict_types=1);
namespace Grafikr;
require_once __DIR__ . '/Runtime.php';

class App
{
    public const SDK_VERSION = '1.1.0';
    public const MAX_FRAME = 8388608;
    public const MAX_OUTSTANDING_REQUESTS = 256;
    public const MAX_PENDING_BYTES = 16777216;
    private $process = null;
    private array $pipes = [];
    private $reader = null;
    private array $readerPipes = [];
    private int $nextId = 0;
    private array $responses = [];
    private array $pendingFibers = [];
    private array $handlers = [];
    private array $events = [];
    private int $eventBytes = 0;
    private bool $dispatchingEvents = false;
    private $errorHandler = null;
    private string $buffer = '';
    private bool $closed = false;
    private int $outstandingRequests = 0;
    private int $pendingBytes = 0;

    public function __construct(?string $host = null)
    {
        $host ??= Runtime::hostPath();
        if (!str_starts_with($host, DIRECTORY_SEPARATOR) && !preg_match('/^[A-Za-z]:[\\\\\/]/', $host)) throw new \InvalidArgumentException('Host path must be absolute');
        $spec = [0 => ['pipe','r'], 1 => ['pipe','w'], 2 => ['pipe','w']];
        $this->process = proc_open([$host], $spec, $this->pipes, null, null, ['bypass_shell' => true]);
        if (!is_resource($this->process)) { $this->closed = true; throw new BridgeException('connection_closed','Could not start Grafikr host'); }
        $reader = dirname(__DIR__) . '/bin/grafikr-reader.php';
        $readerSpec = [0 => $this->pipes[1], 1 => ['pipe','w'], 2 => $this->pipes[2]];
        $this->reader = proc_open([PHP_BINARY, $reader], $readerSpec, $this->readerPipes, null, null, ['bypass_shell' => true]);
        fclose($this->pipes[1]); fclose($this->pipes[2]); unset($this->pipes[1], $this->pipes[2]);
        if (!is_resource($this->reader)) { $this->closed = true; fclose($this->pipes[0]); proc_terminate($this->process); proc_close($this->process); throw new BridgeException('connection_closed','Could not start Grafikr reader relay'); }
        $hello = $this->request('hello',['protocol_major'=>1,'protocol_minor'=>0,'sdk_version'=>self::SDK_VERSION]);
        if (($hello['protocol']['major'] ?? 0) !== 1) { $this->close(); throw new BridgeException('version_mismatch','Protocol 1 required'); }
    }

    public static function start(?RuntimeOptions $options = null): static
    {
        return new static($options?->hostPath ?? Runtime::hostPath($options?->bundle));
    }

    public function request(string $op, array $args = [], float $timeoutSeconds = 30): array
    {
        if ($this->closed) throw new BridgeException('connection_closed','App is closed');
        if ($this->outstandingRequests >= self::MAX_OUTSTANDING_REQUESTS) throw new BridgeException('queue_full','Outstanding request count quota reached');
        $this->outstandingRequests++;
        $reservedBytes = 0;
        try {
            $id=(string)++$this->nextId;
            try { $frame=json_encode(['id'=>$id,'op'=>$op,'args'=>(object)$args],JSON_THROW_ON_ERROR|JSON_UNESCAPED_UNICODE)."\n"; }
            catch (\JsonException $error) { throw new BridgeException('invalid_value','Request arguments are not valid JSON'); }
            if (strlen($frame)>self::MAX_FRAME) throw new BridgeException('frame_too_large','Request exceeds 8 MiB');
            $reservedBytes = strlen($frame);
            if ($this->pendingBytes + $reservedBytes > self::MAX_PENDING_BYTES) throw new BridgeException('queue_full','Outstanding request byte quota reached');
            $this->pendingBytes += $reservedBytes;
            $offset=0; while($offset<strlen($frame)){$written=@fwrite($this->pipes[0],substr($frame,$offset));if($written===false||$written===0)throw new BridgeException('connection_closed','Could not write complete request');$offset+=$written;}
            fflush($this->pipes[0]);
            $fiber = \Fiber::getCurrent();
            if ($fiber !== null) {
                $this->pendingFibers[$id] = $fiber;
                $reply = \Fiber::suspend();
                unset($this->pendingFibers[$id]);
            } else {
                $deadline=microtime(true)+$timeoutSeconds;
                while (!array_key_exists($id,$this->responses)) {
                    if (microtime(true)>=$deadline) throw new BridgeException('timeout','Request timed out');
                    $this->pump(0.1);
                    $status=proc_get_status($this->process);
                    if (!$status['running']) {
                        $error = new BridgeException('connection_closed','Grafikr host exited');
                        $this->fail($error);
                        throw $error;
                    }
                }
                $reply=$this->responses[$id]; unset($this->responses[$id]);
            }
            if (!$reply['ok']) { $error=$reply['error']??[]; throw new BridgeException($error['code']??'internal_failure',$error['message']??'Request failed',$error['field']??null); }
            if ($op !== 'shutdown') $this->dispatchEvents();
            return $reply['result']??[];
        } finally { $this->outstandingRequests--; $this->pendingBytes -= $reservedBytes; }
    }

    public function on(string $node,string $event,callable $callback): Subscription
    {
        $key=$node."\0".$event; $pending="pending:".($this->nextId+1); $this->handlers[$key][$pending]=$callback;
        try { $result=$this->request('event.subscribe',['node'=>$node,'event'=>$event]); $id=(string)$result['subscription']; $this->handlers[$key][$id]=$this->handlers[$key][$pending]; unset($this->handlers[$key][$pending]); return new Subscription($this,$id,$key); }
        catch(\Throwable $error) { unset($this->handlers[$key][$pending]); throw $error; }
    }
    public function onError(callable $handler): void { $this->errorHandler = $handler; }
    public function unsubscribe(string $id,string $key): void { unset($this->handlers[$key][$id]); $this->request('event.unsubscribe',['subscription'=>$id]); }
    public function createWindow(string $title,float $width,float $height): Window { $r=$this->request('window.create',compact('title','width','height')); return new Window($this,(string)$r['window'],(string)$r['root']); }
    public function createNode(Window $window,string $widget,array $properties): Node { $r=$this->request('node.create',['window'=>$window->handle,'widget'=>$widget,'properties'=>(object)$properties]); return new Node($this,(string)$r['node']); }
    public function createState(array $value): State { return new State($this,(string)$this->request('state.create',['value'=>(object)$value])['state']); }
    public function createCollectionProvider(Node $node, array $metadata): CollectionProvider { return CollectionProvider::create($this,$node,$metadata); }
    public function pumpEvents(float $timeoutSeconds = 0.1): void { $this->pump($timeoutSeconds); $this->dispatchEvents(); }

    private function pump(float $seconds): void
    {
        $line=@fgets($this->readerPipes[1],self::MAX_FRAME+1);
        if ($line===false || $line==='') {
            if (feof($this->readerPipes[1])) {
                $error = new BridgeException('connection_closed','Grafikr host closed its output');
                $this->fail($error);
                throw $error;
            }
            return;
        }
        $this->consume($line);
    }
    private function consume(string $chunk): bool
    {
        $this->buffer.=$chunk;
        if (strlen($this->buffer)>self::MAX_FRAME && !str_contains($this->buffer,"\n")) throw new BridgeException('frame_too_large','Host frame exceeds 8 MiB');
        $consumed=false;
        while (($lf=strpos($this->buffer,"\n"))!==false) {
            if ($lf+1>self::MAX_FRAME) throw new BridgeException('frame_too_large','Host frame exceeds 8 MiB');
            $line=substr($this->buffer,0,$lf);$this->buffer=substr($this->buffer,$lf+1);$m=json_decode($line,true,64,JSON_THROW_ON_ERROR);
            if (($m['kind']??'')==='response') {
                $id=(string)($m['id']??'');
                $pendingFiber=$this->pendingFibers[$id]??null;
                if ($pendingFiber !== null) {
                    unset($this->pendingFibers[$id]);
                    try { if ($pendingFiber->isSuspended()) $pendingFiber->resume($m); }
                    catch (\Throwable) {}
                } else $this->responses[$id]=$m;
            }
            elseif (($m['kind']??'')==='event') {
                $size=$lf+1;
                if (count($this->events)>=1024 || $this->eventBytes+$size>16777216) throw new BridgeException('queue_full','Event queue quota reached');
                $this->events[]=[$m,$size]; $this->eventBytes+=$size;
            }
            else throw new BridgeException('invalid_frame','Unknown protocol message kind');
            $consumed=true;
        }
        return $consumed;
    }
    private function dispatchEvents(): void
    {
        if ($this->dispatchingEvents) return;
        $this->dispatchingEvents=true;
        try { while($this->events!==[]) { [$event,$size]=array_shift($this->events); $this->eventBytes-=$size; $key=$event['source']."\0".$event['event'];foreach($this->handlers[$key]??[] as $callback){try{$callback($event);}catch(\Throwable $error){if($this->errorHandler!==null){try{($this->errorHandler)($error,$event);}catch(\Throwable){}}}} } }
        finally { $this->dispatchingEvents=false; }
    }
    public function close(): void
    {
        if (!is_resource($this->process)) {
            foreach (array_merge($this->pipes,$this->readerPipes) as $pipe) if (is_resource($pipe)) fclose($pipe);
            $this->closed=true;
            return;
        }
        $deadline=microtime(true)+5;
        if (!$this->closed && $this->pendingFibers === []) try{$this->request('shutdown',[],max(0.0,$deadline-microtime(true)));}catch(\Throwable){}
        $this->fail(new BridgeException('connection_closed','App is closing'));
        if (isset($this->pipes[0]) && is_resource($this->pipes[0])) fclose($this->pipes[0]);
        while(microtime(true)<$deadline){$status=proc_get_status($this->reader);if(!$status['running'])break;usleep(10000);}
        $status=proc_get_status($this->reader);if($status['running'])proc_terminate($this->reader,9);
        foreach($this->readerPipes as $pipe)if(is_resource($pipe))fclose($pipe);
        proc_close($this->reader); $this->reader=null;
        $status=proc_get_status($this->process);if($status['running'])proc_terminate($this->process,9);
        proc_close($this->process);$this->process=null;$this->closed=true;
    }

    private function fail(BridgeException $error): void
    {
        if ($this->closed) return;
        $this->closed=true;
        $fibers=$this->pendingFibers; $this->pendingFibers=[];
        foreach ($fibers as $pending) {
            $fiber=$pending;
            try { if ($fiber->isSuspended()) $fiber->throw($error); }
            catch (\Throwable) {}
        }
    }

    public function __destruct(){ $this->close(); }
}

final class Subscription { private bool $closed=false; public function __construct(private App $app,private string $id,private string $key){} public function close():void{if(!$this->closed){$this->closed=true;$this->app->unsubscribe($this->id,$this->key);}} }
final class Window { public function __construct(private App $app,public readonly string $handle,public readonly string $root){} public function close():array{return $this->app->request('window.close',['window'=>$this->handle]);} }
final class Node { public function __construct(private App $app,public readonly string $handle){} public function attach(string $parent):array{return $this->app->request('node.attach',['node'=>$this->handle,'parent'=>$parent]);} public function update(array $properties):array{return $this->app->request('property.update',['node'=>$this->handle,'properties'=>(object)$properties]);} public function destroy():array{return $this->app->request('node.destroy',['node'=>$this->handle]);} public function createCollectionProvider(array $metadata):CollectionProvider{return $this->app->createCollectionProvider($this,$metadata);} }
final class State { public function __construct(private App $app,public readonly string $handle){} public function get():array{return $this->app->request('state.get',['state'=>$this->handle]);} public function set(array $value):array{return $this->app->request('state.set',['state'=>$this->handle,'value'=>(object)$value]);} public function close():array{return $this->app->request('state.destroy',['state'=>$this->handle]);} }

final class CollectionRequest
{
    public function __construct(public readonly string $revision,public readonly string $request,public readonly int $start,public readonly int $count) {}
    public static function from(array $event): self
    {
        $payload=$event['payload']??null;
        if (!is_array($payload)||!is_string($payload['revision']??null)||!is_string($payload['request']??null)||!is_int($payload['start']??null)||$payload['start']<0||!is_int($payload['count']??null)||$payload['count']<0||$payload['count']>512)
            throw new BridgeException('invalid_frame','Collection request fields are invalid');
        return new self($payload['revision'],$payload['request'],$payload['start'],$payload['count']);
    }
}

final class CollectionProvider
{
    private \SplQueue $requests;
    private bool $closed=false;
    private ?Subscription $subscription=null;
    private ?BridgeException $failure=null;
    private function __construct(private App $app,private Node $node) { $this->requests=new \SplQueue(); }
    public static function create(App $app,Node $node,array $metadata): self
    {
        self::validateMetadata($metadata);
        $provider=new self($app,$node);
        try {
            $provider->subscription=$app->on($node->handle,'data.request',static function(array $event)use($provider):void{$provider->receive($event);});
            $app->request('collection.create',['node'=>$node->handle]+$metadata);
            return $provider;
        } catch (\Throwable $error) { $provider->close(); throw $error; }
    }
    public function nextRequest(float $timeoutSeconds=30.0): ?CollectionRequest
    {
        $deadline=microtime(true)+max(0.0,$timeoutSeconds);
        while(!$this->closed&&$this->failure===null&&$this->requests->isEmpty()) {
            $remaining=$deadline-microtime(true);
            if($remaining<=0)return null;
            $this->app->pumpEvents(min(0.1,$remaining));
        }
        if(!$this->requests->isEmpty())return $this->requests->dequeue();
        if($this->failure!==null){$error=$this->failure;$this->failure=null;throw $error;}
        return null;
    }
    public function provide(CollectionRequest $request,array $rows): array
    {
        $this->ensureOpen();
        return $this->app->request('data.provide',['node'=>$this->node->handle,'revision'=>$request->revision,'request'=>$request->request,'start'=>$request->start,'rows'=>$rows]);
    }
    public function update(array $metadata): array
    {
        $this->ensureOpen();self::validateMetadata($metadata);
        return $this->app->request('collection.update',['node'=>$this->node->handle,'metadata'=>$metadata]);
    }
    public function close(): void
    {
        if($this->closed)return;$this->closed=true;$this->requests=new \SplQueue();$subscription=$this->subscription;$this->subscription=null;$subscription?->close();
    }
    public function receive(array $event): void
    {
        if($this->closed)return;
        try{$request=CollectionRequest::from($event);}catch(BridgeException $error){$this->failure=$error;return;}
        if($this->requests->count()>=1){$this->failure=new BridgeException('queue_full','Collection provider request queue is full');return;}
        $this->requests->enqueue($request);
    }
    private function ensureOpen(): void { if($this->closed)throw new BridgeException('connection_closed','Collection provider is closed'); }
    private static function validateMetadata(array $metadata): void
    {
        if(!isset($metadata['row_count'],$metadata['row_height'],$metadata['columns'])||!is_int($metadata['row_count'])||$metadata['row_count']<0||(!is_int($metadata['row_height'])&&!is_float($metadata['row_height']))||!is_finite((float)$metadata['row_height'])||$metadata['row_height']<=0||!is_array($metadata['columns'])||array_filter($metadata['columns'],static fn($column)=>!is_string($column))!==[])
            throw new BridgeException('invalid_value','Collection metadata is invalid');
    }
}
