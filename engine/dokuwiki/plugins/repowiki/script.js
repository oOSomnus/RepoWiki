/*
 * RepoWiki DokuWiki plugin scripts.
 * Copyright (C) 2026 RepoWiki contributors.
 * SPDX-License-Identifier: GPL-2.0-or-later
 *
 * Mermaid diagram viewer: clicking a rendered diagram opens it at inspectable
 * size with wheel zooming and drag panning, and every diagram can also be
 * opened as a standalone zoom/pan viewer in a new tab.
 *
 * This integration is distributed under the GNU GPL, version 2 or (at your
 * option) any later version. See LICENSE for the full text.
 */

(function () {
    'use strict';

    var viewer = null;
    var lastFocused = null;

    /* Styles for the standalone viewer document (see buildStandaloneDocument). */
    var STANDALONE_CSS = 'html,body{margin:0;height:100%;background:#fff;color:#17202a;'
        + 'font:16px/1.55 system-ui,sans-serif}'
        + '.repowiki-viewer{position:fixed;inset:0;display:flex;flex-direction:column;background:#fff}'
        + '.repowiki-viewer-bar{display:flex;justify-content:flex-end;gap:.4rem;'
        + 'padding:.6rem .75rem;border-bottom:1px solid #ddd;background:#fafafa}'
        + '.repowiki-viewer-bar button{min-width:2.25rem;padding:.35rem .7rem;'
        + 'border:1px solid #c5ccd4;border-radius:.25rem;background:#fff;color:#17202a;'
        + 'font:inherit;font-size:.95rem;line-height:1.2;cursor:pointer}'
        + '.repowiki-viewer-bar button:hover,.repowiki-viewer-bar button:focus{background:#eef1f5}'
        + '.repowiki-viewer-stage{position:relative;flex:1;overflow:hidden;cursor:grab;touch-action:none}'
        + '.repowiki-viewer-stage.is-dragging{cursor:grabbing}'
        + '.repowiki-viewer-surface{position:absolute;top:0;left:0;transform-origin:0 0;background:#fff}'
        + '.repowiki-viewer-surface svg,.repowiki-viewer-surface svg *{max-width:none}'
        + '.repowiki-viewer-surface svg{display:block;width:auto;height:auto}';

    function naturalSize(svg) {
        var box = svg.viewBox && svg.viewBox.baseVal;
        var width = box && box.width ? box.width : 0;
        var height = box && box.height ? box.height : 0;
        if (!width || !height) {
            var rect = svg.getBoundingClientRect();
            width = rect.width;
            height = rect.height;
        }
        return { width: width || 1, height: height || 1 };
    }

    function escapeHtml(value) {
        return String(value)
            .replace(/&/g, '&amp;')
            .replace(/</g, '&lt;')
            .replace(/>/g, '&gt;')
            .replace(/"/g, '&quot;')
            .replace(/'/g, '&#39;');
    }

    function prepareClone(svg, size) {
        var clone = svg.cloneNode(true);
        clone.style.width = size.width + 'px';
        clone.style.height = size.height + 'px';
        clone.style.maxWidth = 'none';
        clone.removeAttribute('aria-labelledby');
        return clone;
    }

    /*
     * Zero free variables: this function is embedded into generated viewer
     * documents through Function.prototype.toString(). Do not reference the
     * outer scope here, and never write the literal end tag of a script
     * element in this function or its comments.
     */
    function createViewer(doc, options) {
        var MIN_SCALE = 0.05;
        var MAX_SCALE = 16;
        var FIT_SCALE_LIMIT = 2;
        var WHEEL_ZOOM = 1.12;
        var DOUBLE_CLICK_ZOOM = 1.6;

        var width = options.width;
        var height = options.height;
        var svgNode = options.svgNode;
        var state = { scale: 1, x: 0, y: 0, drag: null };

        var root = doc.createElement('div');
        root.className = 'repowiki-viewer';
        if (options.dialog) {
            root.setAttribute('role', 'dialog');
            root.setAttribute('aria-modal', 'true');
            root.setAttribute('aria-label', 'Diagram viewer');
        }

        var bar = doc.createElement('div');
        bar.className = 'repowiki-viewer-bar';

        var buttons = {};
        var actions = options.actions || [];
        actions.forEach(function (entry) {
            var button = doc.createElement('button');
            button.type = 'button';
            button.setAttribute('data-viewer-action', entry.action);
            button.setAttribute('aria-label', entry.label);
            button.title = entry.label;
            button.textContent = entry.text;
            bar.appendChild(button);
            buttons[entry.action] = button;
        });

        var stage = doc.createElement('div');
        stage.className = 'repowiki-viewer-stage';

        var surface = doc.createElement('div');
        surface.className = 'repowiki-viewer-surface';
        surface.style.width = width + 'px';
        surface.style.height = height + 'px';
        surface.appendChild(svgNode);

        stage.appendChild(surface);
        root.appendChild(bar);
        root.appendChild(stage);

        function clampScale(value) {
            return Math.min(MAX_SCALE, Math.max(MIN_SCALE, value));
        }

        function applyTransform() {
            surface.style.transform =
                'translate(' + state.x + 'px, ' + state.y + 'px) scale(' + state.scale + ')';
        }

        function centerView(scale) {
            var box = stage.getBoundingClientRect();
            state.scale = scale;
            state.x = (box.width - width * scale) / 2;
            state.y = (box.height - height * scale) / 2;
            applyTransform();
        }

        function fitScale() {
            var box = stage.getBoundingClientRect();
            var availableWidth = Math.max(box.width - 32, 1);
            var availableHeight = Math.max(box.height - 32, 1);
            return clampScale(
                Math.min(availableWidth / width, availableHeight / height, FIT_SCALE_LIMIT)
            );
        }

        function zoomAt(clientX, clientY, factor) {
            var box = stage.getBoundingClientRect();
            var localX = clientX - box.left;
            var localY = clientY - box.top;
            var next = clampScale(state.scale * factor);
            var ratio = next / state.scale;
            state.x = localX - (localX - state.x) * ratio;
            state.y = localY - (localY - state.y) * ratio;
            state.scale = next;
            applyTransform();
        }

        function handleAction(action) {
            if (action === 'zoom-in') {
                centerView(clampScale(state.scale * 1.25));
            } else if (action === 'zoom-out') {
                centerView(clampScale(state.scale / 1.25));
            } else if (action === 'fit') {
                centerView(fitScale());
            } else if (action === 'actual') {
                centerView(1);
            } else if (options.handlers && typeof options.handlers[action] === 'function') {
                options.handlers[action]();
            }
        }

        actions.forEach(function (entry) {
            buttons[entry.action].addEventListener('click', function () {
                handleAction(entry.action);
            });
        });

        stage.addEventListener('wheel', function (event) {
            event.preventDefault();
            zoomAt(event.clientX, event.clientY, event.deltaY < 0 ? WHEEL_ZOOM : 1 / WHEEL_ZOOM);
        }, { passive: false });

        stage.addEventListener('dblclick', function (event) {
            zoomAt(event.clientX, event.clientY, DOUBLE_CLICK_ZOOM);
        });

        stage.addEventListener('pointerdown', function (event) {
            if (event.button !== 0) return;
            state.drag = {
                x: event.clientX,
                y: event.clientY,
                originX: state.x,
                originY: state.y,
                moved: false
            };
            stage.classList.add('is-dragging');
            if (typeof stage.setPointerCapture === 'function') {
                stage.setPointerCapture(event.pointerId);
            }
        });

        stage.addEventListener('pointermove', function (event) {
            if (!state.drag) return;
            var dx = event.clientX - state.drag.x;
            var dy = event.clientY - state.drag.y;
            if (Math.abs(dx) > 3 || Math.abs(dy) > 3) state.drag.moved = true;
            state.x = state.drag.originX + dx;
            state.y = state.drag.originY + dy;
            applyTransform();
        });

        stage.addEventListener('pointerup', function (event) {
            if (!state.drag) return;
            var moved = state.drag.moved;
            state.drag = null;
            stage.classList.remove('is-dragging');
            if (!moved && event.target === stage && typeof options.onStageTap === 'function') {
                options.onStageTap();
            }
        });

        function onKeyDown(event) {
            if (event.key === 'Escape') {
                if (typeof options.onEscape === 'function') {
                    event.preventDefault();
                    options.onEscape();
                }
                return;
            }
            if (event.key === '+' || event.key === '=') {
                event.preventDefault();
                centerView(clampScale(state.scale * 1.25));
            } else if (event.key === '-' || event.key === '_') {
                event.preventDefault();
                centerView(clampScale(state.scale / 1.25));
            } else if (event.key === '0') {
                event.preventDefault();
                centerView(fitScale());
            }
        }

        doc.addEventListener('keydown', onKeyDown, true);

        return {
            root: root,
            stage: stage,
            surface: surface,
            svgNode: svgNode,
            fit: function () {
                centerView(fitScale());
            },
            destroy: function () {
                doc.removeEventListener('keydown', onKeyDown, true);
                if (root.parentNode) root.parentNode.removeChild(root);
            }
        };
    }

    /*
     * Build the standalone viewer document. The SVG markup is placed in the
     * body (never inside the script text) so diagram labels cannot break out
     * of the bootstrap script.
     */
    function buildStandaloneDocument(svgMarkup, size, title) {
        return '<!doctype html>\n'
            + '<html lang="en">\n<head>\n'
            + '<meta charset="utf-8">\n'
            + '<meta name="viewport" content="width=device-width, initial-scale=1">\n'
            + '<title>' + escapeHtml(title) + '</title>\n'
            + '<style>\n' + STANDALONE_CSS + '\n</style>\n'
            + '</head>\n<body>\n'
            + '<div id="repowiki-diagram-source" hidden>' + svgMarkup + '</div>\n'
            + '<script>\n(function () {\n\'use strict\';\n'
            + createViewer.toString() + '\n'
            + 'var host = document.getElementById(\'repowiki-diagram-source\');\n'
            + 'var api = createViewer(document, {\n'
            + '  svgNode: host.querySelector(\'svg\'),\n'
            + '  width: ' + size.width + ',\n'
            + '  height: ' + size.height + ',\n'
            + '  dialog: false,\n'
            + '  actions: [\n'
            + '    { action: \'zoom-out\', label: \'Zoom out\', text: \'−\' },\n'
            + '    { action: \'zoom-in\', label: \'Zoom in\', text: \'+\' },\n'
            + '    { action: \'fit\', label: \'Fit to window\', text: \'Fit\' },\n'
            + '    { action: \'actual\', label: \'Actual size\', text: \'100%\' }\n'
            + '  ]\n'
            + '});\n'
            + 'document.body.appendChild(api.root);\n'
            + 'api.fit();\n'
            + '})();\n<' + '/script>\n'
            + '</body>\n</html>\n';
    }

    function openStandalone(svg) {
        if (!svg) return;
        var size = naturalSize(svg);
        var clone = prepareClone(svg, size);
        var markup = (typeof XMLSerializer !== 'undefined')
            ? new XMLSerializer().serializeToString(clone)
            : clone.outerHTML;
        var section = svg.closest ? svg.closest('section[data-page-title]') : null;
        var page = (section && section.getAttribute('data-page-title'))
            || document.title || 'Diagram';
        var html = buildStandaloneDocument(markup, size, 'Diagram viewer – ' + page);

        var win = window.open('', '_blank');
        if (win) {
            win.opener = null;
            win.document.write(html);
            win.document.close();
            win.focus();
            return;
        }

        /* Popup blocked: try a blob URL, then fall back to the in-page viewer. */
        var url = null;
        try {
            url = URL.createObjectURL(new Blob([html], { type: 'text/html' }));
            var popup = window.open(url, '_blank');
            if (popup) {
                popup.opener = null;
                setTimeout(function () {
                    URL.revokeObjectURL(url);
                }, 15000);
                return;
            }
        } catch (error) {
            /* Blob URLs are unavailable; fall through to the in-page viewer. */
        }
        if (url) URL.revokeObjectURL(url);
        openViewer(svg);
    }

    function openViewer(svg) {
        if (viewer || !svg) return;
        var size = naturalSize(svg);
        lastFocused = document.activeElement;
        var api = createViewer(document, {
            svgNode: prepareClone(svg, size),
            width: size.width,
            height: size.height,
            dialog: true,
            onEscape: closeViewer,
            onStageTap: closeViewer,
            actions: [
                { action: 'zoom-out', label: 'Zoom out', text: '−' },
                { action: 'zoom-in', label: 'Zoom in', text: '+' },
                { action: 'fit', label: 'Reset view', text: 'Reset' },
                { action: 'open-tab', label: 'Open in new tab', text: '↗' },
                { action: 'close', label: 'Close', text: '×' }
            ],
            handlers: {
                'open-tab': function () {
                    openStandalone(svg);
                },
                'close': closeViewer
            }
        });
        viewer = api;
        document.body.appendChild(api.root);
        api.fit();
        var closeButton = api.root.querySelector('[data-viewer-action="close"]');
        if (closeButton) closeButton.focus();
    }

    function closeViewer() {
        if (!viewer) return;
        viewer.destroy();
        viewer = null;
        if (lastFocused && typeof lastFocused.focus === 'function') {
            lastFocused.focus();
        }
        lastFocused = null;
    }

    /* Add full-screen / new-tab buttons to every diagram container. */
    function injectDiagramActions() {
        var spans = document.querySelectorAll('.mermaid, .mermaidlocked');
        for (var i = 0; i < spans.length; i++) {
            var span = spans[i];
            var container = span.parentNode;
            if (!container || container.querySelector('.repowiki-diagram-actions')) continue;

            var actions = document.createElement('div');
            actions.className = 'repowiki-diagram-actions';

            var full = document.createElement('button');
            full.type = 'button';
            full.setAttribute('data-viewer-action', 'lightbox');
            full.setAttribute('aria-label', 'Open diagram full screen');
            full.title = 'Open full screen';
            full.textContent = '⤢';

            var tab = document.createElement('button');
            tab.type = 'button';
            tab.setAttribute('data-viewer-action', 'open-tab');
            tab.setAttribute('aria-label', 'Open diagram in a new tab');
            tab.title = 'Open in new tab';
            tab.textContent = '↗';

            (function (source) {
                full.addEventListener('click', function () {
                    openViewer(source.querySelector('svg'));
                });
                tab.addEventListener('click', function () {
                    openStandalone(source.querySelector('svg'));
                });
            })(span);

            actions.appendChild(full);
            actions.appendChild(tab);
            container.appendChild(actions);
        }
    }

    document.addEventListener('click', function (event) {
        var target = event.target;
        if (!target || typeof target.closest !== 'function') return;
        if (target.closest('.repowiki-viewer') || target.closest('.repowiki-diagram-actions')) {
            return;
        }
        var svg = target.closest('.mermaid svg, .mermaidlocked svg');
        if (!svg) return;
        event.preventDefault();
        openViewer(svg);
    });

    if (document.readyState === 'loading') {
        document.addEventListener('DOMContentLoaded', injectDiagramActions);
    } else {
        injectDiagramActions();
    }
})();
