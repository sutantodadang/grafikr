<?php
declare(strict_types=1);

$buffer = '';
while (!feof(STDIN)) {
    $chunk = fread(STDIN, 8192);
    if ($chunk === false || $chunk === '') break;
    $buffer .= $chunk;
    while (($lf = strpos($buffer, "\n")) !== false) {
        if ($lf + 1 > 8388608) { fwrite(STDERR, "Grafikr host frame exceeds 8 MiB\n"); exit(2); }
        $offset = 0;
        $frame = substr($buffer, 0, $lf + 1);
        while ($offset < strlen($frame)) {
            $written = fwrite(STDOUT, substr($frame, $offset));
            if ($written === false || $written === 0) exit(0);
            $offset += $written;
        }
        fflush(STDOUT);
        $buffer = substr($buffer, $lf + 1);
    }
    if (strlen($buffer) >= 8388608) { fwrite(STDERR, "Grafikr host frame exceeds 8 MiB\n"); exit(2); }
}
if ($buffer !== '') { fwrite(STDERR, "Grafikr host ended in a partial frame\n"); exit(2); }
