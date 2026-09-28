<?php

/*
 * RepoWiki DokuWiki plugin support.
 * Copyright (C) 2026 RepoWiki contributors.
 * SPDX-License-Identifier: GPL-2.0-or-later
 *
 * This integration is distributed under the GNU GPL, version 2 or (at your
 * option) any later version. See LICENSE for the full text.
 */

/**
 * Reads and renders the Rust-supplied edition catalog without deriving page IDs.
 */
final class RepoWikiCatalog
{
    private const MAX_CATALOG_BYTES = 8_388_608;
    private static ?array $catalog = null;
    private static bool $loaded = false;

    /**
     * @return array|null A decoded catalog, or null when none is available.
     */
    public static function load(): ?array
    {
        global $conf;
        if (self::$loaded) return self::$catalog;
        self::$loaded = true;

        $savedir = $conf['savedir'] ?? null;
        if (!is_string($savedir) || $savedir === '') return null;
        $file = rtrim($savedir, '/\\') . DIRECTORY_SEPARATOR . 'repowiki_catalog.json';
        if (is_link($file) || !is_file($file) || !is_readable($file)) return null;
        $size = filesize($file);
        if ($size === false || $size > self::MAX_CATALOG_BYTES) return null;

        try {
            $decoded = json_decode((string)file_get_contents($file), true, 512, JSON_THROW_ON_ERROR);
        } catch (Throwable) {
            return null;
        }
        if (!is_array($decoded) || !isset($decoded['editions']) || !is_array($decoded['editions'])) return null;
        self::$catalog = $decoded;
        return self::$catalog;
    }

    /**
     * Find the edition containing this exact supplied page ID. For pages not
     * explicitly listed, use the catalog's declared default edition.
     */
    public static function activeEdition(?string $pageId = null): ?array
    {
        $catalog = self::load();
        if ($catalog === null) return null;
        if ($pageId === null) {
            global $ID;
            $pageId = is_string($ID ?? null) ? $ID : '';
        }

        foreach ($catalog['editions'] as $edition) {
            if (!is_array($edition) || !is_string($edition['wiki_id'] ?? null)) continue;
            if (($edition['start_id'] ?? null) === $pageId || self::treeContains($edition['tree'] ?? [], $pageId)) {
                return $edition;
            }
        }

        $default = $catalog['default_edition'] ?? null;
        foreach ($catalog['editions'] as $edition) {
            if (!is_array($edition)) continue;
            if (($edition['id'] ?? null) === $default || ($edition['wiki_id'] ?? null) === $default) {
                return $edition;
            }
        }
        foreach ($catalog['editions'] as $edition) {
            if (is_array($edition)) return $edition;
        }
        return null;
    }

    /** Render the navigation and edition controls using only catalog IDs. */
    public static function renderNavigation(): string
    {
        global $ID;
        $catalog = self::load();
        $active = self::activeEdition(is_string($ID ?? null) ? $ID : '');
        if ($catalog === null || $active === null) return '';

        $currentId = is_string($ID ?? null) ? $ID : '';
        $html = '<style>' . self::styles() . '</style>';
        $html .= '<nav class="repowiki-navigation" data-repowiki-navigation aria-label="RepoWiki navigation">';
        $html .= '<div class="repowiki-controls">';
        $html .= '<form class="repowiki-editions" method="get" action="' . self::escape(script()) . '">';
        $html .= '<input type="hidden" name="do" value="show">';
        $html .= '<label>Edition <select name="id">';
        foreach ($catalog['editions'] as $edition) {
            if (!is_array($edition) || !is_string($edition['start_id'] ?? null)) continue;
            $title = self::editionLabel($edition);
            $selected = (($edition['wiki_id'] ?? null) === ($active['wiki_id'] ?? null)) ? ' selected' : '';
            $html .= '<option value="' . self::escape($edition['start_id']) . '"' . $selected . '>' . self::escape($title) . '</option>';
        }
        $html .= '</select></label><button type="submit">Open edition</button></form>';
        $html .= '<form class="repowiki-search" role="search" method="get" action="' . self::escape(script()) . '">';
        $html .= '<input type="hidden" name="do" value="search">';
        $html .= '<input type="hidden" name="id" value="' . self::escape($currentId) . '">';
        $html .= '<label><span>Search this edition</span><input type="search" name="q" placeholder="Search this edition"></label>';
        $html .= '<button type="submit">Search</button></form></div>';
        $html .= '<h2>' . self::escape(self::editionLabel($active)) . '</h2>';
        $html .= self::renderTree(is_array($active['tree'] ?? null) ? $active['tree'] : [], $currentId, 0);
        $html .= '</nav>';
        return $html;
    }

    /** @param mixed $nodes */
    private static function treeContains($nodes, string $pageId, int $depth = 0): bool
    {
        if (!is_array($nodes) || $depth > 128) return false;
        foreach ($nodes as $node) {
            if (!is_array($node)) continue;
            if (($node['page_id'] ?? null) === $pageId) return true;
            if (self::treeContains($node['children'] ?? [], $pageId, $depth + 1)) return true;
        }
        return false;
    }

    /** @param array<int, mixed> $nodes */
    private static function renderTree(array $nodes, string $currentId, int $depth): string
    {
        if ($nodes === [] || $depth > 128) return '';
        $html = '<ul>';
        foreach ($nodes as $node) {
            if (!is_array($node) || !is_string($node['page_id'] ?? null)) continue;
            $pageId = $node['page_id'];
            $title = is_string($node['title'] ?? null) ? $node['title'] : $pageId;
            $class = $pageId === $currentId ? ' class="current" aria-current="page"' : '';
            $url = wl($pageId, '', false, '&');
            $html .= '<li><a' . $class . ' href="' . self::escape($url) . '">' . self::escape($title) . '</a>';
            $children = $node['children'] ?? [];
            if (is_array($children) && $children !== []) {
                $html .= self::renderTree($children, $currentId, $depth + 1);
            }
            $html .= '</li>';
        }
        return $html . '</ul>';
    }

    private static function editionLabel(array $edition): string
    {
        foreach (['label', 'title', 'wiki_id'] as $field) {
            if (is_string($edition[$field] ?? null) && $edition[$field] !== '') return $edition[$field];
        }
        return 'Wiki edition';
    }

    private static function escape(string $value): string
    {
        return htmlspecialchars($value, ENT_QUOTES | ENT_SUBSTITUTE | ENT_HTML5, 'UTF-8');
    }

    private static function styles(): string
    {
        return '.repowiki-navigation{border:1px solid #ddd;border-radius:.3rem;margin:0 0 1.5rem;padding:1rem}' .
            '.repowiki-controls{display:flex;flex-wrap:wrap;gap:.75rem;align-items:end}' .
            '.repowiki-controls form,.repowiki-controls label{display:flex;flex-wrap:wrap;gap:.35rem;align-items:center}' .
            '.repowiki-navigation ul{margin:.35rem 0 .35rem 1.25rem}' .
            '.repowiki-navigation a.current{font-weight:bold}' .
            '.repowiki-navigation h2{font-size:1.1rem;margin:.8rem 0 .3rem}' .
            '.repowiki-search label span{position:absolute;clip:rect(0,0,0,0)}';
    }
}
