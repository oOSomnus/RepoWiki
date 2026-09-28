#!/usr/bin/env php
<?php

/*
 * RepoWiki DokuWiki CLI adapter.
 * Copyright (C) 2026 RepoWiki contributors.
 * SPDX-License-Identifier: GPL-2.0-or-later
 *
 * This integration is distributed under the GNU GPL, version 2 or (at your
 * option) any later version. See ../plugins/repowiki/LICENSE for the full text.
 */

function repowiki_cli_error(string $message): never
{
    fwrite(STDERR, "repowiki: $message\n");
    exit(1);
}

if (PHP_VERSION_ID < 80200) {
    repowiki_cli_error('PHP 8.2 or newer is required');
}
if (PHP_SAPI !== 'cli') {
    repowiki_cli_error('this adapter must be invoked from the command line');
}
if ($argc !== 2) {
    repowiki_cli_error('expected one request JSON file path');
}

try {
    require_once dirname(__DIR__) . '/bootstrap.php';
    repowiki_dokuwiki_define_paths();
} catch (Throwable $error) {
    repowiki_cli_error($error->getMessage());
}

// The upstream dispatcher consumes the plugin name as its first positional
// argument. Keep the public interface to a single request-file argument.
$argv = [$argv[0], 'repowiki', $argv[1]];
$_SERVER['argv'] = $argv;
$_SERVER['argc'] = count($argv);

// Give parser/rendering helpers the same public script context as DokuWiki,
// rather than the adapter's filesystem path.
$_SERVER['SCRIPT_NAME'] = '/doku.php';
$_SERVER['PHP_SELF'] = '/doku.php';
$_SERVER['SCRIPT_FILENAME'] = DOKU_INC . 'doku.php';

// DokuWiki setup errors may call exit() after writing an HTML message. Drop
// any unconsumed buffered output and report that failure on stderr instead.
$repowikiCliBufferLevel = ob_get_level() + 1;
$repowikiCliResponseForwarded = false;
register_shutdown_function(static function () use (&$repowikiCliBufferLevel, &$repowikiCliResponseForwarded): void {
    if ($repowikiCliResponseForwarded || ob_get_level() < $repowikiCliBufferLevel) return;
    $bufferedOutput = '';
    while (ob_get_level() >= $repowikiCliBufferLevel) {
        $bufferedOutput = ob_get_clean() . $bufferedOutput;
    }
    $detail = trim(html_entity_decode(strip_tags($bufferedOutput), ENT_QUOTES | ENT_HTML5, 'UTF-8'));
    $detail = preg_replace('/\\s+/', ' ', $detail);
    if (is_string($detail) && $detail !== '') {
        fwrite(STDERR, 'repowiki: ' . substr($detail, 0, 2048) . "\\n");
    } else {
        fwrite(STDERR, "repowiki: DokuWiki terminated before returning a JSON response\\n");
    }
});
ob_start();
require DOKU_INC . 'bin/plugin.php';
$output = ob_get_clean();

try {
    $response = json_decode($output, false, 512, JSON_THROW_ON_ERROR);
    if (!$response instanceof stdClass) {
        throw new RuntimeException('CLI plugin did not return a JSON object');
    }
} catch (Throwable $error) {
    repowiki_cli_error('invalid CLI plugin response: ' . $error->getMessage());
}

$repowikiCliResponseForwarded = true;
echo json_encode($response, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR) . "\n";
