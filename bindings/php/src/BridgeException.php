<?php
declare(strict_types=1);
namespace Grafikr;

final class BridgeException extends \RuntimeException
{
    public function __construct(public readonly string $codeName, string $message, public readonly ?string $field = null) { parent::__construct($message); }
}
