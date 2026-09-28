<?php

/*
 * RepoWiki DokuWiki syntax plugin.
 * Copyright (C) 2026 RepoWiki contributors.
 * SPDX-License-Identifier: GPL-2.0-or-later
 *
 * This integration is distributed under the GNU GPL, version 2 or (at your
 * option) any later version. See LICENSE for the full text.
 */

use dokuwiki\Extension\SyntaxPlugin;
use dokuwiki\Parsing\Handler;
use dokuwiki\Parsing\ModeRegistry;

require_once __DIR__ . '/lib.php';

class syntax_plugin_repowiki extends SyntaxPlugin
{
    /** @inheritdoc */
    public function getType()
    {
        return ModeRegistry::CATEGORY_SUBSTITUTION;
    }

    /** @inheritdoc */
    public function getPType()
    {
        return 'block';
    }

    /** @inheritdoc */
    public function getSort()
    {
        return 155;
    }

    /** @inheritdoc */
    public function connectTo($mode)
    {
        $this->Lexer->addSpecialPattern('~~REPOWIKI_NAV~~', $mode, 'plugin_repowiki');
    }

    /** @inheritdoc */
    public function handle($match, $state, $pos, Handler $handler)
    {
        return [];
    }

    /** @inheritdoc */
    public function render($format, Doku_Renderer $renderer, $data)
    {
        if ($format !== 'xhtml') return false;
        $renderer->doc .= RepoWikiCatalog::renderNavigation();
        return true;
    }
}
