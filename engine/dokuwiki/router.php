<?php

/*
 * RepoWiki DokuWiki built-in-server router.
 * Copyright (C) 2026 RepoWiki contributors.
 * SPDX-License-Identifier: GPL-2.0-or-later
 *
 * This integration is distributed under the GNU GPL, version 2 or (at your
 * option) any later version. See plugins/repowiki/LICENSE for the full text.
 */

if (PHP_VERSION_ID < 80200) {
    error_log('RepoWiki DokuWiki router: PHP 8.2 or newer is required');
    http_response_code(503);
    header('Content-Type: text/plain; charset=UTF-8');
    echo "PHP 8.2 or newer is required\n";
    return true;
}

function repowiki_router_reply(int $status, string $message): never
{
    http_response_code($status);
    header('Content-Type: text/plain; charset=UTF-8');
    header('X-Content-Type-Options: nosniff');
    echo $message . "\n";
    exit;
}

$method = strtoupper($_SERVER['REQUEST_METHOD'] ?? 'GET');
if ($method !== 'GET' && $method !== 'HEAD') {
    header('Allow: GET, HEAD');
    repowiki_router_reply(405, 'Method not allowed');
}

$requestUri = $_SERVER['REQUEST_URI'] ?? '/';
$rawPath = parse_url($requestUri, PHP_URL_PATH);
if (!is_string($rawPath) || $rawPath === '' || $rawPath[0] !== '/') {
    repowiki_router_reply(400, 'Invalid request path');
}
$path = rawurldecode($rawPath);
if (str_contains($path, "\0") || str_contains($path, '\\') || str_contains($path, '%')) {
    repowiki_router_reply(400, 'Invalid request path');
}
foreach (explode('/', $path) as $segment) {
    if ($segment === '.' || $segment === '..') repowiki_router_reply(400, 'Invalid request path');
}

$application = getenv('REPOWIKI_DOKUWIKI_DIR');
if ($application === false || $application === '') {
    error_log('RepoWiki DokuWiki router: REPOWIKI_DOKUWIKI_DIR is not set');
    repowiki_router_reply(500, 'DokuWiki runtime is unavailable');
}
$applicationRoot = realpath($application);
if ($applicationRoot === false || !is_file($applicationRoot . '/doku.php')) {
    error_log('RepoWiki DokuWiki router: invalid DokuWiki application root');
    repowiki_router_reply(500, 'DokuWiki runtime is unavailable');
}

if ($path === '/') $path = '/doku.php';
$dynamicRoutes = [
    '/doku.php',
    '/lib/exe/ajax.php',
    '/lib/exe/css.php',
    '/lib/exe/detail.php',
    '/lib/exe/fetch.php',
    '/lib/exe/indexer.php',
    '/lib/exe/jquery.php',
    '/lib/exe/js.php',
    '/lib/exe/manifest.php',
    '/lib/exe/opensearch.php',
];

if (in_array($path, $dynamicRoutes, true)) {
    try {
        require_once __DIR__ . '/bootstrap.php';
        repowiki_dokuwiki_define_paths();
    } catch (Throwable $error) {
        error_log('RepoWiki DokuWiki router: ' . $error->getMessage());
        repowiki_router_reply(500, 'DokuWiki runtime is unavailable');
    }

    $relativePath = ltrim($path, '/');
    $script = realpath(DOKU_INC . $relativePath);
    $rootPrefix = rtrim(DOKU_INC, DIRECTORY_SEPARATOR) . DIRECTORY_SEPARATOR;
    if ($script === false || !str_starts_with($script, $rootPrefix) || !is_file($script)) {
        repowiki_router_reply(404, 'Not found');
    }

    $_SERVER['SCRIPT_NAME'] = $path;
    $_SERVER['PHP_SELF'] = $path;
    $_SERVER['SCRIPT_FILENAME'] = $script;
    require $script;
    return true;
}

// Only the public asset trees are static. In particular, no PHP file under
// lib/plugins or any other non-front-controller path is ever dispatched.
$publicAssetRoot = false;
foreach (['/lib/images/', '/lib/scripts/', '/lib/tpl/', '/lib/plugins/'] as $prefix) {
    if (str_starts_with($path, $prefix)) {
        $publicAssetRoot = true;
        break;
    }
}
if ($path === '/favicon.ico') $publicAssetRoot = true;
$extension = strtolower(pathinfo($path, PATHINFO_EXTENSION));
$publicAssetExtensions = ['css', 'eot', 'gif', 'ico', 'jpeg', 'jpg', 'js', 'png', 'svg', 'ttf', 'webp', 'woff', 'woff2'];
if (!$publicAssetRoot || !in_array($extension, $publicAssetExtensions, true)) {
    repowiki_router_reply(404, 'Not found');
}

$staticFile = realpath($applicationRoot . $path);
$rootPrefix = rtrim($applicationRoot, DIRECTORY_SEPARATOR) . DIRECTORY_SEPARATOR;
if ($staticFile === false || !str_starts_with($staticFile, $rootPrefix) || !is_file($staticFile)) {
    repowiki_router_reply(404, 'Not found');
}

header('X-Content-Type-Options: nosniff');
return false;
