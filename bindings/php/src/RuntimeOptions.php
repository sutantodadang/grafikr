<?php
declare(strict_types=1);
namespace Grafikr;

final readonly class RuntimeOptions
{
    public function __construct(public ?string $bundle = null, public ?string $hostPath = null) {}
}
