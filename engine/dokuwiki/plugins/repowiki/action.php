<?php

/*
 * RepoWiki DokuWiki action plugin.
 * Copyright (C) 2026 RepoWiki contributors.
 * SPDX-License-Identifier: GPL-2.0-or-later
 *
 * This integration is distributed under the GNU GPL, version 2 or (at your
 * option) any later version. See LICENSE for the full text.
 */

use dokuwiki\Extension\ActionPlugin;
use dokuwiki\Extension\Event;
use dokuwiki\Extension\EventHandler;

require_once __DIR__ . '/lib.php';

class action_plugin_repowiki extends ActionPlugin
{
    /** @inheritdoc */
    public function register(EventHandler $controller)
    {
        $controller->register_hook('DOKUWIKI_STARTED', 'BEFORE', $this, 'serveCatalog');
        $controller->register_hook('TPL_CONTENT_DISPLAY', 'BEFORE', $this, 'addNavigation');
        $controller->register_hook('SEARCH_QUERY_PAGELOOKUP', 'AFTER', $this, 'scopeSearchResults');
        $controller->register_hook('SEARCH_QUERY_FULLPAGE', 'AFTER', $this, 'scopeSearchResults');
    }

    /** Serve the exact savedir catalog before DokuWiki starts its HTML template. */
    public function serveCatalog(Event $event, $param): void
    {
        global $ACT;
        if (($ACT ?? null) !== 'repowiki_catalog') return;

        $catalog = RepoWikiCatalog::load();
        header('Content-Type: application/json; charset=utf-8');
        header('Cache-Control: no-store');
        if ($catalog === null) {
            http_response_code(404);
            echo json_encode(['error' => 'RepoWiki catalog is unavailable'], JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE);
        } else {
            echo json_encode($catalog, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR);
        }
        if (session_status() === PHP_SESSION_ACTIVE) session_write_close();
        exit;
    }

    /**
     * Add the ancestor path above the page content, and only fall back to the
     * full navigation tree when the template has no sidebar to host it.
     *
     * The bundled template shows its aside only while a page is displayed, so
     * actions such as search still need the tree within the content area.
     */
    public function addNavigation(Event $event, $param): void
    {
        global $ACT;
        if (!is_string($event->data)) return;
        if (str_contains($event->data, 'data-repowiki-path') || str_contains($event->data, 'data-repowiki-navigation')) {
            return;
        }

        $showingPage = ($ACT ?? null) === 'show';
        $content = $showingPage ? RepoWikiCatalog::renderPath() : '';
        if (!($showingPage && RepoWikiCatalog::sidebarAvailable())) {
            $content .= RepoWikiCatalog::renderNavigation();
        }
        if ($content !== '') $event->data = $content . $event->data;
    }

    /**
     * Restrict both native title lookup and full-text results to one edition.
     * This keeps DokuWiki's own search route/rendering while preventing results
     * from a sibling edition, even when a query contains another namespace.
     */
    public function scopeSearchResults(Event $event, $param): void
    {
        $edition = RepoWikiCatalog::activeEdition();
        if ($edition === null || !is_string($edition['wiki_id'] ?? null) || !is_array($event->result)) {
            $event->result = [];
            return;
        }

        $namespace = $edition['wiki_id'] . ':';
        $event->result = array_filter(
            $event->result,
            static fn($value, $pageId): bool => is_string($pageId) && str_starts_with($pageId, $namespace),
            ARRAY_FILTER_USE_BOTH
        );
    }
}
