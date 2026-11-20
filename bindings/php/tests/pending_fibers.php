<?php
declare(strict_types=1);
require_once __DIR__.'/../src/App.php';
require_once __DIR__.'/../src/BridgeException.php';

$host = getenv('GRAFIKR_FIXTURE_HOST');
if (!$host) throw new RuntimeException('fixture host required');
$app = new Grafikr\App($host);

$fibers = [];
$rejected = [];
for ($i = 0; $i < Grafikr\App::MAX_OUTSTANDING_REQUESTS; $i++) {
    $fiber = new Fiber(static function () use ($app, &$rejected, $i): void {
        try { $app->request('state.get', ['state'=>'0'], 0.5); }
        catch (Grafikr\BridgeException $error) { if ($error->codeName === 'connection_closed') $rejected[$i] = ($rejected[$i] ?? 0) + 1; }
    });
    $fiber->start();
    if (!$fiber->isSuspended()) throw new RuntimeException('fiber request blocked instead of becoming pending');
    $fibers[] = $fiber;
}
$overflow = new Fiber(static function () use ($app): bool {
    try { $app->request('state.get', ['state'=>'0'], 0.5); }
    catch (Grafikr\BridgeException $error) { return $error->codeName === 'queue_full'; }
    return false;
});
$overflow->start();
if (!$overflow->isTerminated() || $overflow->getReturn() !== true) throw new RuntimeException('257th pending fiber was not rejected with queue_full');

$sawEof = false;
try { $app->pumpEvents(0.1); }
catch (Grafikr\BridgeException $error) { $sawEof = $error->codeName === 'connection_closed'; }
if (!$sawEof) throw new RuntimeException('host EOF was not reported as connection_closed');
if (count($rejected) !== Grafikr\App::MAX_OUTSTANDING_REQUESTS || array_sum($rejected) !== Grafikr\App::MAX_OUTSTANDING_REQUESTS) throw new RuntimeException('EOF did not reject each pending fiber exactly once');
if (array_filter($fibers, static fn(Fiber $fiber): bool => !$fiber->isTerminated()) !== []) throw new RuntimeException('EOF left request fibers suspended');
$app->close();
echo "Grafikr PHP pending Fiber EOF checks passed\n";
