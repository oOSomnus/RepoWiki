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
    /** @var string[] parser instructions that say something about the page source itself */
    private const STRUCTURAL_INSTRUCTIONS = ['header', 'code', 'file', 'unformatted'];

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
                        'structure' => $this->pageStructure($instructions, $content),
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
     * Project the parser's structural instructions back onto the page source.
     *
     * DokuWiki lexes "\n" . <source with CRLF folded to LF> . "\n" (Parser::parse),
     * so every instruction position sits one byte past the same offset in the
     * source. The offsets reported here undo that shift and are byte offsets into
     * the CRLF-folded source, not character indices: a caller slicing text with
     * them has to fold line endings the same way and round to a character
     * boundary first, or a page with multibyte text cuts a character in half.
     *
     * @param array<int, array> $instructions
     * @return array{headings: list<array<string, mixed>>, spans: list<array<string, mixed>>}
     */
    private function pageStructure(array $instructions, string $content): array
    {
        $source = str_replace("\r\n", "\n", $content);
        $headings = [];
        $spans = [];
        foreach ($instructions as $instruction) {
            if (!is_array($instruction)) continue;
            $name = $instruction[0] ?? null;
            if (!is_string($name) || !in_array($name, self::STRUCTURAL_INSTRUCTIONS, true)) {
                continue;
            }
            $arguments = $instruction[1] ?? null;
            $position = $instruction[2] ?? null;
            if (!is_array($arguments) || !is_int($position)) {
                throw new RuntimeException("DokuWiki parser returned an invalid $name instruction");
            }

            if ($name === 'header') {
                $headings[] = $this->headingStructure($arguments, $position, $source);
            } else {
                $spans[] = $this->spanStructure($name, $arguments, $position, $source);
            }
        }
        return ['headings' => $headings, 'spans' => $spans];
    }

    /**
     * @param array<int, mixed> $arguments header title, level and source position
     * @return array<string, mixed>
     */
    private function headingStructure(array $arguments, int $position, string $source): array
    {
        $title = $arguments[0] ?? null;
        $level = $arguments[1] ?? null;
        if (!is_string($title) || !is_int($level)) {
            throw new RuntimeException('DokuWiki parser returned an invalid header instruction');
        }

        // The heading pattern claims a whole line and only whitespace may sit
        // beside it, so the heading runs from its line start to that line's end.
        $start = $this->sourcePosition($position);
        $newline = strpos($source, "\n", $start);
        return [
            'level' => $level,
            'text' => $title,
            'start' => $start,
            'end' => $newline === false ? strlen($source) : $newline,
        ];
    }

    /**
     * @param array<int, mixed> $arguments the unparsed body of a code, file or unformatted run
     * @return array<string, mixed>
     */
    private function spanStructure(string $name, array $arguments, int $position, string $source): array
    {
        $body = $arguments[0] ?? null;
        if (!is_string($body)) {
            throw new RuntimeException("DokuWiki parser returned an invalid $name instruction");
        }
        $bodyStart = $this->sourcePosition($position);
        if ($name === 'unformatted') {
            [$opener, $closer, $kind] = $this->unformattedDelimiters($source, $bodyStart);
        } else {
            [$opener, $closer, $kind] = ['<' . $name, '</' . $name . '>', $name];
        }

        // The lexer reports the body alone, bounded exactly by the two patterns
        // that opened and closed the mode, so the delimiters are recoverable from
        // the body's own position: the opener ends where the body starts, and the
        // closer is the first occurrence of the exit pattern at or after it.
        // For <code> and <file> this holds even though the open tag carries
        // attributes: the entry pattern ('<code\b(?=.*</code>)') matches only the
        // 5 bytes "<code" — \b and the lookahead consume nothing — so the body
        // token still begins with the tag's remainder (" java>") and its position
        // sits exactly strlen($opener) past the tag start. The substr guard below
        // is what turns any other lexer behaviour into a loud failure.
        $start = $bodyStart - strlen($opener);
        if ($start < 0 || substr($source, $start, strlen($opener)) !== $opener) {
            throw new RuntimeException("DokuWiki {$name} body at {$bodyStart} is not preceded by {$opener}");
        }
        $close = strpos($source, $closer, $bodyStart);
        if ($close === false) {
            throw new RuntimeException("DokuWiki {$name} body at {$bodyStart} is not closed by {$closer}");
        }
        return ['kind' => $kind, 'start' => $start, 'end' => $close + strlen($closer)];
    }

    /**
     * <nowiki> and %% both suppress markup and both report the same instruction
     * name, so only the bytes beside the body say which of them opened the run.
     *
     * @return array{0: string, 1: string, 2: string} opener, closer and reported kind
     */
    private function unformattedDelimiters(string $source, int $bodyStart): array
    {
        $tag = '<nowiki>';
        if ($bodyStart >= strlen($tag)
            && substr($source, $bodyStart - strlen($tag), strlen($tag)) === $tag) {
            return [$tag, '</nowiki>', 'nowiki'];
        }
        return ['%%', '%%', 'unformatted'];
    }

    /**
     * Undo the newline DokuWiki's parser prepends to the source before lexing it.
     */
    private function sourcePosition(int $position): int
    {
        if ($position < 1) {
            throw new RuntimeException(
                "DokuWiki instruction position $position is before the page source"
            );
        }
        return $position - 1;
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
