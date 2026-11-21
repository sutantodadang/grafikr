<?php
require __DIR__.'/../src/BridgeException.php';
require __DIR__.'/../src/App.php';
final class CollectionTestApp extends Grafikr\App
{
    public array $calls = [];
    private $callback = null;
    public function __construct() {}
    public function request(string $op, array $args = [], float $timeoutSeconds = 30): array { $this->calls[] = [$op, $args]; return []; }
    public function on(string $node, string $event, callable $callback): Grafikr\Subscription
    {
        $this->calls[] = ['event.subscribe', ['node'=>$node,'event'=>$event]];
        $this->callback = $callback;
        $callback(['payload'=>['revision'=>'4','request'=>'9','start'=>3,'count'=>2]]);
        return new Grafikr\Subscription($this, 'sub-1', $node."\0".$event);
    }
    public function unsubscribe(string $id, string $key): void { $this->calls[] = ['event.unsubscribe', ['subscription'=>$id]]; }
    public function pumpEvents(float $timeoutSeconds = 0.1): void
    {
        if ($this->callback !== null) ($this->callback)(['payload'=>['revision'=>'4','request'=>'10','start'=>5,'count'=>1]]);
    }
}

function assertCollectionProvider(): void
{
    $app = new CollectionTestApp();
    $node = new Grafikr\Node($app, '7');
    $provider = $app->createCollectionProvider($node, ['row_count'=>100,'row_height'=>24,'columns'=>['name']]);
    $request = $provider->nextRequest(0);
    if ($request?->start !== 3 || $request->count !== 2) throw new RuntimeException('PHP collection request was lost');
    $provider->provide($request, [['Ada'],['Lin']]);
    $provider->update(['row_count'=>2,'row_height'=>24,'columns'=>['name']]);
    $later = $provider->nextRequest(0.1);
    if ($later?->request !== '10') throw new RuntimeException('PHP collection wait did not pump host events');
    $overflowEvent=['payload'=>['revision'=>'4','request'=>'11','start'=>6,'count'=>1]];
    $provider->receive($overflowEvent);$provider->receive($overflowEvent);
    if($provider->nextRequest(0)?->request!=='11')throw new RuntimeException('PHP queued row request was lost');
    try{$provider->nextRequest(0);throw new RuntimeException('PHP collection overflow was hidden');}
    catch(Grafikr\BridgeException $error){if($error->codeName!=='queue_full')throw $error;}
    $expected = ['event.subscribe','collection.create','data.provide','collection.update'];
    if (array_column($app->calls, 0) !== $expected) throw new RuntimeException('PHP collection operation order mismatch');
    $provider->close();
}

function sampleWidgetValue(string $widget, string $name, string $type, string $state, string $image): mixed
{
    if ($widget === 'Radio' && $name === 'group') return $state;
    if ($widget === 'TreeView' && $name === 'selected') return 'root';
    if ($widget === 'Slider') return ['min'=>0.0,'max'=>10.0,'width'=>120.0][$name] ?? 1.0;
    if ($widget === 'Spinner' && $name === 'size') return 16.0;
    if ($name === 'row_count') return 1;
    if ($name === 'row_height') return 24.0;
    return match ($type) {
        'number', 'f32' => 1.0,
        'u16' => 1,
        'u64' => 0,
        'string' => 'sample',
        'color' => '#336699',
        'transform' => ['x'=>0.0,'y'=>0.0,'scale'=>1.0],
        'boolean' => false,
        'primary|secondary|icon' => 'primary',
        'state' => $state,
        'GridColumn[]' => [['title'=>'Column','width'=>120.0]],
        'string[]' => ['One','Two'],
        'TreeNode[]' => [['key'=>'root','label'=>'Root','children'=>[]]],
        'CanvasCommand[]' => [['kind'=>'rect','rect'=>['x'=>0.0,'y'=>0.0,'width'=>1.0,'height'=>1.0],'color'=>'#336699','radius'=>0.0]],
        'ImageHandle' => $image,
        default => throw new RuntimeException("No PHP conformance value for {$widget}.{$name}: {$type}"),
    };
}

function assertWidgetInventory(string $host): void
{
    $widgets = json_decode(file_get_contents(dirname(__DIR__, 2).'/protocol/v1/widgets.json'), true, flags: JSON_THROW_ON_ERROR)['widgets'];
    $app = new Grafikr\App($host);
    try {
        $window = $app->createWindow('widget inventory', 640, 480);
        $state = $app->createState(['type'=>'i64','value'=>'1']);
        $upload = $app->request('image.begin', ['width'=>1,'height'=>1,'byte_length'=>4])['upload'];
        $app->request('image.chunk', ['upload'=>$upload,'offset'=>0,'data'=>base64_encode("\0\0\0\xff")]);
        $image = $app->request('image.finalize', ['upload'=>$upload])['image'];

        foreach ($widgets as $widget => $spec) {
            $properties = [];
            foreach ($spec['properties'] as $name => $type) $properties[$name] = sampleWidgetValue($widget,$name,$type,$state->handle,$image);
            $node = $app->createNode($window,$widget,$properties);
            $node->attach($window->root);
            foreach ($spec['events'] ?? [] as $eventName) {
                $received = null;
                $subscription = $app->on($node->handle,$eventName,static function(array $event) use (&$received): void {$received=$event;});
                if (($received['source'] ?? null) !== $node->handle || ($received['event'] ?? null) !== $eventName) {
                    throw new RuntimeException("{$widget}@{$eventName} was not delivered");
                }
                $subscription->close();
            }
            $node->destroy();
        }
        $app->request('image.destroy',['image'=>$image]);
        $state->close();
        $window->close();
    } finally {
        $app->close();
    }
}

assertCollectionProvider();
function assertRuntimeBundleChecks(): void
{
    $target = (new ReflectionMethod(Grafikr\Runtime::class, 'target'))->invoke(null);
    $root = sys_get_temp_dir().DIRECTORY_SEPARATOR.'grafikr-runtime-test-'.bin2hex(random_bytes(8));
    mkdir($root); mkdir($root.DIRECTORY_SEPARATOR.'bin');
    $host = $root.DIRECTORY_SEPARATOR.'bin'.DIRECTORY_SEPARATOR.'grafikr-host';
    $manifestPath = $root.DIRECTORY_SEPARATOR.'manifest.json';
    try {
        file_put_contents($host, 'runtime host fixture');
        $manifest = ['engine_version'=>Grafikr\App::SDK_VERSION,'target'=>$target,'development_fixture'=>false,'files'=>[[
            'path'=>'bin/grafikr-host','size'=>filesize($host),'sha256'=>hash_file('sha256',$host),'role'=>'host',
        ]]];
        file_put_contents($manifestPath, json_encode($manifest, JSON_THROW_ON_ERROR));
        if (Grafikr\Runtime::hostPath($root) !== realpath($host)) throw new RuntimeException('verified PHP runtime path mismatch');
        file_put_contents($host, 'tampered');
        try { Grafikr\Runtime::hostPath($root); throw new RuntimeException('tampered PHP runtime was accepted'); }
        catch (Grafikr\BridgeException $error) { if ($error->codeName !== 'runtime_checksum') throw $error; }
        $manifest['files'][0]['path'] = '../outside';
        file_put_contents($manifestPath, json_encode($manifest, JSON_THROW_ON_ERROR));
        try { Grafikr\Runtime::hostPath($root); throw new RuntimeException('unsafe PHP runtime path was accepted'); }
        catch (Grafikr\BridgeException $error) { if ($error->codeName !== 'runtime_path') throw $error; }
    } finally {
        @unlink($manifestPath); @unlink($host); @rmdir($root.DIRECTORY_SEPARATOR.'bin'); @rmdir($root);
    }
}
assertRuntimeBundleChecks();
assert(Grafikr\App::MAX_FRAME === 8388608);
$limited = (new ReflectionClass(Grafikr\App::class))->newInstanceWithoutConstructor();
(new ReflectionProperty(Grafikr\App::class, 'outstandingRequests'))->setValue($limited, 256);
try { $limited->request('hello'); throw new RuntimeException('request count limit was not enforced'); }
catch (Grafikr\BridgeException $error) { if ($error->codeName !== 'queue_full') throw $error; }
if (!defined(Grafikr\App::class.'::MAX_PENDING_BYTES')) throw new RuntimeException('request byte quota is not defined');
$byteLimited = (new ReflectionClass(Grafikr\App::class))->newInstanceWithoutConstructor();
(new ReflectionProperty(Grafikr\App::class, 'pendingBytes'))->setValue($byteLimited, 16 * 1024 * 1024);
try { $byteLimited->request('hello'); throw new RuntimeException('request byte limit was not enforced'); }
catch (Grafikr\BridgeException $error) { if ($error->codeName !== 'queue_full') throw $error; }
$invalidValue = (new ReflectionClass(Grafikr\App::class))->newInstanceWithoutConstructor();
try { $invalidValue->request('state.create', ['value'=>['type'=>'f64','value'=>NAN]]); throw new RuntimeException('non-finite state was accepted'); }
catch (Grafikr\BridgeException $error) { if ($error->codeName !== 'invalid_value') throw $error; }
$correlation = (new ReflectionClass(Grafikr\App::class))->newInstanceWithoutConstructor();
(new ReflectionProperty(Grafikr\App::class, 'buffer'))->setValue($correlation, '');
(new ReflectionProperty(Grafikr\App::class, 'responses'))->setValue($correlation, []);
$consume = new ReflectionMethod(Grafikr\App::class, 'consume');
$consume->invoke($correlation,
    json_encode(['kind'=>'response','id'=>'second','ok'=>true,'result'=>['value'=>'second']])."\n".
    json_encode(['kind'=>'response','id'=>'first','ok'=>true,'result'=>['value'=>'first']])."\n");
$correlated = (new ReflectionProperty(Grafikr\App::class, 'responses'))->getValue($correlation);
if (($correlated['first']['result']['value'] ?? null) !== 'first' || ($correlated['second']['result']['value'] ?? null) !== 'second') throw new RuntimeException('Out-of-order responses lost their request correlation');
$eventQueue = (new ReflectionClass(Grafikr\App::class))->newInstanceWithoutConstructor();
(new ReflectionProperty(Grafikr\App::class, 'buffer'))->setValue($eventQueue, '');
(new ReflectionProperty(Grafikr\App::class, 'responses'))->setValue($eventQueue, []);
(new ReflectionProperty(Grafikr\App::class, 'events'))->setValue($eventQueue, []);
(new ReflectionProperty(Grafikr\App::class, 'eventBytes'))->setValue($eventQueue, 0);
$eventBurst = '';
for ($sequence=1;$sequence<=1025;$sequence++) $eventBurst .= json_encode(['kind'=>'event','sequence'=>(string)$sequence,'event'=>'click','source'=>'7','payload'=>[]])."\n";
try { $consume->invoke($eventQueue,$eventBurst); throw new RuntimeException('blocked callback event queue was unbounded'); }
catch (Grafikr\BridgeException $error) { if ($error->codeName !== 'queue_full') throw $error; }
if (count((new ReflectionProperty(Grafikr\App::class, 'events'))->getValue($eventQueue)) !== 1024) throw new RuntimeException('event queue exceeded its message bound');
$byteQueue = (new ReflectionClass(Grafikr\App::class))->newInstanceWithoutConstructor();
(new ReflectionProperty(Grafikr\App::class, 'buffer'))->setValue($byteQueue, '');
(new ReflectionProperty(Grafikr\App::class, 'responses'))->setValue($byteQueue, []);
(new ReflectionProperty(Grafikr\App::class, 'events'))->setValue($byteQueue, []);
(new ReflectionProperty(Grafikr\App::class, 'eventBytes'))->setValue($byteQueue, 16 * 1024 * 1024);
try { $consume->invoke($byteQueue,json_encode(['kind'=>'event','sequence'=>'1','event'=>'click','source'=>'7','payload'=>[]])."\n"); throw new RuntimeException('event byte queue was unbounded'); }
catch (Grafikr\BridgeException $error) { if ($error->codeName !== 'queue_full') throw $error; }
$executable = PHP_OS_FAMILY === 'Windows' ? 'fixture-host.exe' : 'fixture-host';
$host = getenv('GRAFIKR_FIXTURE_HOST') ?: dirname(__DIR__, 3).DIRECTORY_SEPARATOR.'target'.DIRECTORY_SEPARATOR.'debug'.DIRECTORY_SEPARATOR.$executable;
$app = new Grafikr\App($host);
$window = $app->createWindow('fixture', 320, 240);
$button = $app->createNode($window, 'Button', ['label' => 'click']);
$button->attach($window->root);
$received = false;
$nested = false;
$subscription = $app->on($button->handle, 'click', static function (array $event) use (&$received, &$nested, $button, $app): void {
    $received = ($event['source'] ?? null) === $button->handle && ($event['event'] ?? null) === 'click';
    if ($received) {
        $state = $app->request('state.create', ['value'=>['type'=>'i64','value'=>'1']])['state'] ?? null;
        $nested = is_string($state);
        if ($nested) $app->request('state.destroy', ['state'=>$state]);
    }
});
if (!$received || !$nested) throw new RuntimeException('Early subscription event or nested callback request failed');
$callbackError = false;
$app->onError(static function (Throwable $error, array $event) use (&$callbackError): void {
    $callbackError = $error->getMessage() === 'callback failed' && ($event['event'] ?? null) === 'click';
});
$failedCallback = $app->on($button->handle, 'click', static function (): void { throw new RuntimeException('callback failed'); });
if (!$callbackError) throw new RuntimeException('callback exception did not reach the PHP error handler');
$failedCallback->close();
$list = $app->createNode($window, 'VirtualList', ['row_count'=>1,'row_height'=>24,'height'=>120]);
$list->attach($window->root);
$collection = $app->createCollectionProvider($list, ['row_count'=>1,'row_height'=>24,'columns'=>['label']]);
$range = $collection->nextRequest(2);
if ($range === null || $range->count !== 1) throw new RuntimeException('visible collection range was not delivered');
$collection->provide($range, [['row']]);
$collection->close();
$subscription->close();

$text = "line one\nline two 雪 😀".str_repeat('x', 102400);
$textState = $app->createState(['type'=>'string','value'=>$text]);
$textResult = $app->request('state.get', ['state'=>$textState->handle]);
if (($textResult['value'] ?? null) !== $text || ($textResult['type'] ?? null) !== 'string') throw new RuntimeException('Large UTF-8 state frame did not round trip');
$integer = $app->createState(['type'=>'i64','value'=>'-9223372036854775808']);
if (($app->request('state.get',['state'=>$integer->handle])['value'] ?? null) !== '-9223372036854775808') throw new RuntimeException('i64 minimum did not round trip');
$app->request('state.set',['state'=>$integer->handle,'value'=>['type'=>'i64','value'=>'9223372036854775807']]);
if (($app->request('state.get',['state'=>$integer->handle])['value'] ?? null) !== '9223372036854775807') throw new RuntimeException('i64 maximum did not round trip');

$unsubscribeEvents = 0;
$lateSubscription = $app->on($button->handle,'click',static function() use (&$unsubscribeEvents): void {$unsubscribeEvents++;});
if ($unsubscribeEvents !== 1) throw new RuntimeException('Early event was not delivered to the new listener');
$lateSubscription->close();
if ($unsubscribeEvents !== 1) throw new RuntimeException('Event was dispatched after unsubscribe began');
$app->close();
if (getenv('GRAFIKR_FIXTURE_WIDGET_INVENTORY') !== false) {
    $collectionMode = getenv('GRAFIKR_FIXTURE_COLLECTION_REQUEST');
    putenv('GRAFIKR_FIXTURE_COLLECTION_REQUEST');
    try { assertWidgetInventory($host); }
    finally { if ($collectionMode !== false) putenv('GRAFIKR_FIXTURE_COLLECTION_REQUEST='.$collectionMode); }
}

$sentinelMarker = tempnam(sys_get_temp_dir(),'grafikr-sentinel-');
unlink($sentinelMarker);
$sentinel = proc_open([PHP_BINARY,'-r','usleep(300000); file_put_contents($argv[1], "alive");',$sentinelMarker],[['pipe','r'],['pipe','w'],['pipe','w']],$sentinelPipes);
$independentApp = new Grafikr\App($host);
$independentApp->close();
proc_close($sentinel);
if (@file_get_contents($sentinelMarker) !== 'alive') throw new RuntimeException('Closing Grafikr terminated the unrelated sentinel process');
unlink($sentinelMarker);
echo "Grafikr PHP protocol checks passed\n";
