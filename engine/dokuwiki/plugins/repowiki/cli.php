<?php

/*
 * RepoWiki DokuWiki CLI plugin.
 * Copyright (C) 2026 RepoWiki contributors.
 * SPDX-License-Identifier: GPL-2.0-or-later
 *
 * This integration is distributed under the GNU GPL, version 2 or (at your
 * option) any later version. See LICENSE for the full text.
 */

use dokuwiki\Extension\CLIPlugin;
use dokuwiki\File\PageFile;
use dokuwiki\File\PageResolver;
use splitbrain\phpcli\Options;

class cli_plugin_repowiki extends CLIPlugin
{
    /** @inheritdoc */
    protected function setup(Options $options)
    {
        $options->setHelp('Read, write, parse, and render RepoWiki pages through DokuWiki.');
        $options->registerArgument('request-json-path', 'Path to the RepoWiki JSON request file', true);
    }

    /** @inheritdoc */
    protected function main(Options $options)
    {
        try {
            $arguments = $options->getArgs();
            if (count($arguments) !== 1) {
                throw new RuntimeException('expected one request JSON file path');
            }

            $requestPath = $arguments[0];
            if (!is_file($requestPath) || !is_readable($requestPath)) {
                throw new RuntimeException('request JSON file is not readable');
            }
            $requestBytes = file_get_contents($requestPath);
            if ($requestBytes === false) {
                throw new RuntimeException('unable to read request JSON file');
            }

            $request = json_decode($requestBytes, false, 512, JSON_THROW_ON_ERROR);
            if (!$request instanceof stdClass) {
                throw new RuntimeException('request JSON must be an object');
            }

            $operation = $request->op ?? null;
            if (!is_string($operation)) {
                throw new RuntimeException('request field "op" must be a string');
            }
            $pageId = $request->page_id ?? null;
            if (!is_string($pageId) || $pageId === '' || str_contains($pageId, "\0")) {
                throw new RuntimeException('request field "page_id" must be a non-empty DokuWiki page ID');
            }
            if (cleanID($pageId) !== $pageId) {
                throw new RuntimeException('page_id must already be a canonical DokuWiki page ID');
            }

            $page = new PageFile($pageId);
            switch ($operation) {
                case 'read':
                    $result = ['content' => $page->rawWikiText()];
                    break;

                case 'write':
                    $content = $this->requireContent($request);
                    $page->saveWikiText($content, 'RepoWiki generated or updated page', false);
                    $result = ['success' => true];
                    break;

                case 'parse':
                    $content = $this->requireContent($request);
                    $instructions = $this->parseForPage($content, $pageId);
                    $result = [
                        'links' => $this->internalTargets($instructions, $pageId),
                        'html' => $this->renderInstructions($instructions),
                    ];
                    break;

                case 'render':
                    $content = $page->rawWikiText();
                    $instructions = $this->parseForPage($content, $pageId);
                    $result = ['html' => $this->renderInstructions($instructions)];
                    break;

                default:
                    throw new RuntimeException('unsupported operation: ' . $operation);
            }

            echo json_encode($result, JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR) . "\n";
        } catch (Throwable $error) {
            fwrite(STDERR, 'repowiki: ' . $error->getMessage() . "\n");
            exit(1);
        }
    }

    private function requireContent(stdClass $request): string
    {
        if (!property_exists($request, 'content') || !is_string($request->content)) {
            throw new RuntimeException('request field "content" must be a string for this operation');
        }
        return $request->content;
    }

    /**
     * Invoke DokuWiki's configured parser with the supplied page as context.
     *
     * @return array<int, array>
     */
    private function parseForPage(string $content, string $pageId): array
    {
        global $ID;
        $ID = $pageId;
        $instructions = p_get_instructions($content);
        if (!is_array($instructions)) {
            throw new RuntimeException('DokuWiki parser did not return instructions');
        }
        return $instructions;
    }

    /**
     * @param array<int, array> $instructions
     * @return string[] resolved DokuWiki page IDs for all internal-link instructions
     */
    private function internalTargets(array $instructions, string $pageId): array
    {
        $resolver = new PageResolver($pageId);
        $targets = [];
        foreach ($instructions as $instruction) {
            if (!is_array($instruction) || ($instruction[0] ?? null) !== 'internallink') continue;
            $arguments = $instruction[1] ?? null;
            if (!is_array($arguments) || !isset($arguments[0]) || !is_string($arguments[0])) {
                throw new RuntimeException('DokuWiki parser returned an invalid internal-link instruction');
            }

            // PageResolver applies DokuWiki's actual relative-link, start-page,
            // useslash, and cleanID semantics; PHP does not invent module IDs.
            $resolved = $resolver->resolveId($arguments[0]);
            $target = strstr($resolved, '#', true);
            if ($target === false) $target = $resolved;
            if ($target !== '') $targets[] = $target;
        }
        return $targets;
    }

    /**
     * @param array<int, array> $instructions
     */
    private function renderInstructions(array $instructions): string
    {
        $info = [];
        $html = p_render('xhtml', $instructions, $info);
        if (!is_string($html)) {
            throw new RuntimeException('DokuWiki XHTML renderer failed');
        }
        return $html;
    }
}
