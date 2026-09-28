<?php

/*
 * RepoWiki DokuWiki integration bootstrap.
 * Copyright (C) 2026 RepoWiki contributors.
 * SPDX-License-Identifier: GPL-2.0-or-later
 *
 * This integration is distributed under the GNU GPL, version 2 or (at your
 * option) any later version. See plugins/repowiki/LICENSE for the full text.
 */

/**
 * Establish the fixed DokuWiki application, isolated config, and plugin overlay.
 *
 * @throws RuntimeException when a required runtime root is unavailable
 */
function repowiki_dokuwiki_define_paths(): void
{
    $paths = [
        'REPOWIKI_DOKUWIKI_DIR' => 'DOKU_INC',
        'REPOWIKI_DOKU_CONF' => 'DOKU_CONF',
        'REPOWIKI_DOKU_PLUGIN_DIR' => 'DOKU_PLUGIN',
    ];

    $resolved = [];
    foreach ($paths as $environmentVariable => $constant) {
        $environmentPath = getenv($environmentVariable);
        if ($environmentPath === false || $environmentPath === '') {
            throw new RuntimeException("Required environment variable $environmentVariable is not set");
        }

        $path = realpath($environmentPath);
        if ($path === false || !is_dir($path)) {
            throw new RuntimeException("$environmentVariable does not name an accessible directory");
        }
        $resolved[$constant] = rtrim($path, DIRECTORY_SEPARATOR) . DIRECTORY_SEPARATOR;
    }

    if (!is_file($resolved['DOKU_INC'] . 'doku.php') || !is_file($resolved['DOKU_INC'] . 'bin/plugin.php')) {
        throw new RuntimeException('REPOWIKI_DOKUWIKI_DIR is not a DokuWiki application root');
    }
    if (!is_file($resolved['DOKU_CONF'] . 'dokuwiki.php')) {
        throw new RuntimeException('REPOWIKI_DOKU_CONF has no DokuWiki configuration');
    }
    if (!is_file($resolved['DOKU_PLUGIN'] . 'repowiki/cli.php') ||
        !is_file($resolved['DOKU_PLUGIN'] . 'repowiki/action.php') ||
        !is_file($resolved['DOKU_PLUGIN'] . 'repowiki/syntax.php')) {
        throw new RuntimeException('REPOWIKI_DOKU_PLUGIN_DIR does not contain the RepoWiki plugin overlay');
    }

    foreach ($resolved as $constant => $path) {
        if (!defined($constant)) define($constant, $path);
    }
}
