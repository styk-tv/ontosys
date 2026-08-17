//! # Serve Command
//!
//! Start the visualization web server.

use super::*;
use std::path::Path;
use std::fs;
use std::net::SocketAddr;
use console::style;
use axum::{
    Router,
    routing::get,
    response::{Html, Json},
    extract::State,
};
use tower_http::cors::CorsLayer;
use std::sync::Arc;

struct AppState {
    ontosys_path: std::path::PathBuf,
}

pub async fn run(repo_path: &Path, port: u16, open_browser: bool) -> anyhow::Result<()> {
    println!("\n{}", style("OntoSys Visualization Server").cyan().bold());
    println!("{}\n", style("═".repeat(50)).dim());

    let git_root = find_git_root(repo_path).ok_or_else(|| {
        anyhow::anyhow!("Not a git repository")
    })?;

    let ontosys_path = ontosys_dir(&git_root);
    if !ontosys_path.exists() {
        error("Not initialized. Run 'ontosys init' first.");
        return Err(anyhow::anyhow!("Not initialized"));
    }

    let graph_path = ontosys_path.join("data/graph.json");
    if !graph_path.exists() {
        error("No graph data found. Run 'ontosys build' first.");
        return Err(anyhow::anyhow!("No graph data"));
    }

    // Create the visualization HTML
    let viz_html = generate_visualization_html();
    let viz_path = ontosys_path.join("viz/index.html");
    fs::write(&viz_path, &viz_html)?;
    info("Generated visualization HTML");

    let state = Arc::new(AppState {
        ontosys_path: ontosys_path.clone(),
    });

    // Build router
    let app = Router::new()
        .route("/", get(serve_index))
        .route("/api/graph", get(serve_graph_data))
        .route("/api/stats", get(serve_stats))
        .layer(CorsLayer::permissive())
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));

    println!();
    success(&format!("Server starting on http://localhost:{}", port));
    println!();
    println!("  {} Open {} in your browser", style("→").dim(), style(format!("http://localhost:{}", port)).cyan().underlined());
    println!("  {} Press {} to stop", style("→").dim(), style("Ctrl+C").yellow());
    println!();

    if open_browser {
        let _ = open::that(format!("http://localhost:{}", port));
    }

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

async fn serve_index(State(state): State<Arc<AppState>>) -> Html<String> {
    let viz_path = state.ontosys_path.join("viz/index.html");
    let html = fs::read_to_string(&viz_path).unwrap_or_else(|_| generate_visualization_html());
    Html(html)
}

async fn serve_graph_data(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let graph_path = state.ontosys_path.join("data/graph.json");
    let data = fs::read_to_string(&graph_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({"nodes": [], "edges": []}));
    Json(data)
}

async fn serve_stats(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let meta_path = state.ontosys_path.join("data/build-meta.json");
    let data = fs::read_to_string(&meta_path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or(serde_json::json!({}));
    Json(data)
}

fn generate_visualization_html() -> String {
    r##"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>OntoSys - Code Knowledge Graph</title>
    <script src="https://cdnjs.cloudflare.com/ajax/libs/d3/7.8.5/d3.min.js"></script>
    <style>
        * {
            margin: 0;
            padding: 0;
            box-sizing: border-box;
        }

        body {
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: #0d1117;
            color: #c9d1d9;
            overflow: hidden;
        }

        #app {
            display: flex;
            height: 100vh;
            width: 100vw;
        }

        #sidebar {
            width: 320px;
            background: #161b22;
            border-right: 1px solid #30363d;
            display: flex;
            flex-direction: column;
            overflow: hidden;
        }

        #sidebar-header {
            padding: 16px;
            border-bottom: 1px solid #30363d;
        }

        #sidebar-header h1 {
            font-size: 18px;
            font-weight: 600;
            color: #58a6ff;
            display: flex;
            align-items: center;
            gap: 8px;
        }

        #stats {
            padding: 12px 16px;
            background: #21262d;
            font-size: 13px;
            display: flex;
            gap: 16px;
        }

        .stat {
            display: flex;
            flex-direction: column;
        }

        .stat-value {
            font-size: 20px;
            font-weight: 600;
            color: #58a6ff;
        }

        .stat-label {
            color: #8b949e;
            font-size: 11px;
        }

        #filters {
            padding: 16px;
            border-bottom: 1px solid #30363d;
        }

        #filters h3 {
            font-size: 12px;
            font-weight: 600;
            color: #8b949e;
            margin-bottom: 12px;
            text-transform: uppercase;
        }

        .filter-group {
            margin-bottom: 16px;
        }

        .filter-group label {
            display: block;
            font-size: 13px;
            margin-bottom: 6px;
            color: #c9d1d9;
        }

        input[type="text"], select {
            width: 100%;
            padding: 8px 12px;
            background: #0d1117;
            border: 1px solid #30363d;
            border-radius: 6px;
            color: #c9d1d9;
            font-size: 14px;
        }

        input[type="text"]:focus, select:focus {
            outline: none;
            border-color: #58a6ff;
        }

        .checkbox-group {
            display: flex;
            flex-wrap: wrap;
            gap: 8px;
        }

        .checkbox-item {
            display: flex;
            align-items: center;
            gap: 6px;
            padding: 4px 10px;
            background: #21262d;
            border-radius: 16px;
            font-size: 12px;
            cursor: pointer;
            transition: all 0.2s;
        }

        .checkbox-item:hover {
            background: #30363d;
        }

        .checkbox-item.active {
            background: #238636;
            color: white;
        }

        .checkbox-item input {
            display: none;
        }

        #node-details {
            flex: 1;
            overflow-y: auto;
            padding: 16px;
        }

        #node-details h3 {
            font-size: 12px;
            font-weight: 600;
            color: #8b949e;
            margin-bottom: 12px;
            text-transform: uppercase;
        }

        .detail-card {
            background: #21262d;
            border-radius: 8px;
            padding: 16px;
            margin-bottom: 12px;
        }

        .detail-card h4 {
            font-size: 16px;
            color: #58a6ff;
            margin-bottom: 8px;
            word-break: break-all;
        }

        .detail-card .type-badge {
            display: inline-block;
            padding: 2px 8px;
            background: #238636;
            border-radius: 12px;
            font-size: 11px;
            font-weight: 600;
            margin-bottom: 12px;
        }

        .detail-props {
            font-size: 13px;
        }

        .detail-props dt {
            color: #8b949e;
            margin-top: 8px;
        }

        .detail-props dd {
            color: #c9d1d9;
            word-break: break-all;
        }

        #graph-container {
            flex: 1;
            position: relative;
        }

        #graph {
            width: 100%;
            height: 100%;
        }

        .node {
            cursor: pointer;
        }

        .node circle {
            stroke: #30363d;
            stroke-width: 2px;
            transition: all 0.2s;
        }

        .node:hover circle {
            stroke: #58a6ff;
            stroke-width: 3px;
        }

        .node.selected circle {
            stroke: #f0883e;
            stroke-width: 3px;
        }

        .node text {
            font-size: 10px;
            fill: #8b949e;
            pointer-events: none;
        }

        .link {
            stroke: #30363d;
            stroke-opacity: 0.6;
            fill: none;
        }

        .link.highlighted {
            stroke: #58a6ff;
            stroke-opacity: 1;
            stroke-width: 2px;
        }

        #controls {
            position: absolute;
            bottom: 16px;
            right: 16px;
            display: flex;
            gap: 8px;
        }

        .control-btn {
            padding: 8px 12px;
            background: #21262d;
            border: 1px solid #30363d;
            border-radius: 6px;
            color: #c9d1d9;
            cursor: pointer;
            font-size: 13px;
            transition: all 0.2s;
        }

        .control-btn:hover {
            background: #30363d;
            border-color: #58a6ff;
        }

        #loading {
            position: absolute;
            top: 50%;
            left: 50%;
            transform: translate(-50%, -50%);
            text-align: center;
        }

        .spinner {
            width: 40px;
            height: 40px;
            border: 3px solid #30363d;
            border-top-color: #58a6ff;
            border-radius: 50%;
            animation: spin 1s linear infinite;
            margin: 0 auto 16px;
        }

        @keyframes spin {
            to { transform: rotate(360deg); }
        }

        /* Node type colors */
        .node-Function circle { fill: #238636; }
        .node-Struct circle { fill: #58a6ff; }
        .node-Class circle { fill: #58a6ff; }
        .node-Trait circle { fill: #a371f7; }
        .node-Interface circle { fill: #a371f7; }
        .node-Module circle { fill: #f0883e; }
        .node-Enum circle { fill: #d29922; }
        .node-Import circle { fill: #8b949e; }
        .node-Project circle { fill: #f85149; }
        .node-unknown circle { fill: #484f58; }
    </style>
</head>
<body>
    <div id="app">
        <div id="sidebar">
            <div id="sidebar-header">
                <h1>
                    <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2">
                        <circle cx="12" cy="12" r="3"/>
                        <circle cx="19" cy="5" r="2"/>
                        <circle cx="5" cy="5" r="2"/>
                        <circle cx="19" cy="19" r="2"/>
                        <circle cx="5" cy="19" r="2"/>
                        <line x1="12" y1="9" x2="12" y2="7"/>
                        <line x1="9.5" y1="13.5" x2="6" y2="17"/>
                        <line x1="14.5" y1="13.5" x2="18" y2="17"/>
                        <line x1="6" y1="7" x2="9" y2="10"/>
                        <line x1="18" y1="7" x2="15" y2="10"/>
                    </svg>
                    OntoSys
                </h1>
            </div>

            <div id="stats">
                <div class="stat">
                    <span class="stat-value" id="node-count">-</span>
                    <span class="stat-label">Nodes</span>
                </div>
                <div class="stat">
                    <span class="stat-value" id="edge-count">-</span>
                    <span class="stat-label">Edges</span>
                </div>
                <div class="stat">
                    <span class="stat-value" id="type-count">-</span>
                    <span class="stat-label">Types</span>
                </div>
            </div>

            <div id="filters">
                <h3>Filters</h3>

                <div class="filter-group">
                    <label>Search</label>
                    <input type="text" id="search" placeholder="Search nodes...">
                </div>

                <div class="filter-group">
                    <label>Node Types</label>
                    <div class="checkbox-group" id="type-filters">
                        <!-- Populated by JS -->
                    </div>
                </div>
            </div>

            <div id="node-details">
                <h3>Selected Node</h3>
                <div id="selected-node-info">
                    <p style="color: #8b949e; font-size: 13px;">Click a node to see details</p>
                </div>
            </div>
        </div>

        <div id="graph-container">
            <div id="loading">
                <div class="spinner"></div>
                <p>Loading graph...</p>
            </div>
            <svg id="graph"></svg>
            <div id="controls">
                <button class="control-btn" onclick="resetZoom()">Reset View</button>
                <button class="control-btn" onclick="toggleLabels()">Toggle Labels</button>
            </div>
        </div>
    </div>

    <script>
        let graphData = { nodes: [], edges: [] };
        let simulation;
        let svg, g;
        let nodeElements, linkElements;
        let showLabels = true;
        let selectedNode = null;
        let activeTypes = new Set();
        let zoom;

        // Color map for node types
        const typeColors = {
            'Function': '#238636',
            'Struct': '#58a6ff',
            'Class': '#58a6ff',
            'Trait': '#a371f7',
            'Interface': '#a371f7',
            'Module': '#f0883e',
            'Enum': '#d29922',
            'Import': '#8b949e',
            'Project': '#f85149',
            'unknown': '#484f58'
        };

        async function loadData() {
            try {
                const response = await fetch('/api/graph');
                graphData = await response.json();

                document.getElementById('loading').style.display = 'none';

                // Update stats
                document.getElementById('node-count').textContent = graphData.nodes?.length || 0;
                document.getElementById('edge-count').textContent = graphData.edges?.length || 0;

                // Get unique types
                const types = new Set(graphData.nodes?.map(n => n.type) || []);
                document.getElementById('type-count').textContent = types.size;

                // Initialize type filters
                initTypeFilters(types);

                // Initialize graph
                initGraph();
            } catch (err) {
                console.error('Failed to load graph:', err);
                document.getElementById('loading').innerHTML = '<p style="color: #f85149;">Failed to load graph data</p>';
            }
        }

        function initTypeFilters(types) {
            const container = document.getElementById('type-filters');
            container.innerHTML = '';

            types.forEach(type => {
                activeTypes.add(type);

                const item = document.createElement('label');
                item.className = 'checkbox-item active';
                item.innerHTML = `
                    <input type="checkbox" checked data-type="${type}">
                    <span style="width: 8px; height: 8px; border-radius: 50%; background: ${typeColors[type] || typeColors.unknown}"></span>
                    ${type}
                `;

                item.addEventListener('click', () => {
                    const checkbox = item.querySelector('input');
                    checkbox.checked = !checkbox.checked;
                    item.classList.toggle('active', checkbox.checked);

                    if (checkbox.checked) {
                        activeTypes.add(type);
                    } else {
                        activeTypes.delete(type);
                    }

                    updateVisibility();
                });

                container.appendChild(item);
            });
        }

        function initGraph() {
            const container = document.getElementById('graph-container');
            const width = container.clientWidth;
            const height = container.clientHeight;

            svg = d3.select('#graph')
                .attr('width', width)
                .attr('height', height);

            // Add zoom
            zoom = d3.zoom()
                .scaleExtent([0.1, 4])
                .on('zoom', (event) => {
                    g.attr('transform', event.transform);
                });

            svg.call(zoom);

            g = svg.append('g');

            // Create links
            linkElements = g.append('g')
                .selectAll('line')
                .data(graphData.edges || [])
                .enter()
                .append('line')
                .attr('class', 'link')
                .attr('stroke-width', 1);

            // Create nodes
            nodeElements = g.append('g')
                .selectAll('g')
                .data(graphData.nodes || [])
                .enter()
                .append('g')
                .attr('class', d => `node node-${d.type}`)
                .call(d3.drag()
                    .on('start', dragStarted)
                    .on('drag', dragged)
                    .on('end', dragEnded))
                .on('click', nodeClicked);

            nodeElements.append('circle')
                .attr('r', d => getNodeRadius(d))
                .attr('fill', d => typeColors[d.type] || typeColors.unknown);

            nodeElements.append('text')
                .text(d => d.label)
                .attr('dx', d => getNodeRadius(d) + 4)
                .attr('dy', 3);

            // Create simulation
            simulation = d3.forceSimulation(graphData.nodes || [])
                .force('link', d3.forceLink(graphData.edges || [])
                    .id(d => d.id)
                    .distance(80))
                .force('charge', d3.forceManyBody().strength(-200))
                .force('center', d3.forceCenter(width / 2, height / 2))
                .force('collision', d3.forceCollide().radius(30))
                .on('tick', ticked);

            // Search handler
            document.getElementById('search').addEventListener('input', (e) => {
                const query = e.target.value.toLowerCase();
                updateVisibility(query);
            });
        }

        function getNodeRadius(node) {
            const type = node.type;
            if (type === 'Project') return 20;
            if (type === 'Module') return 15;
            if (type === 'Class' || type === 'Struct') return 12;
            return 8;
        }

        function ticked() {
            linkElements
                .attr('x1', d => d.source.x)
                .attr('y1', d => d.source.y)
                .attr('x2', d => d.target.x)
                .attr('y2', d => d.target.y);

            nodeElements.attr('transform', d => `translate(${d.x},${d.y})`);
        }

        function dragStarted(event) {
            if (!event.active) simulation.alphaTarget(0.3).restart();
            event.subject.fx = event.subject.x;
            event.subject.fy = event.subject.y;
        }

        function dragged(event) {
            event.subject.fx = event.x;
            event.subject.fy = event.y;
        }

        function dragEnded(event) {
            if (!event.active) simulation.alphaTarget(0);
            event.subject.fx = null;
            event.subject.fy = null;
        }

        function nodeClicked(event, d) {
            // Deselect previous
            if (selectedNode) {
                d3.select(selectedNode).classed('selected', false);
            }

            // Select new
            selectedNode = event.currentTarget;
            d3.select(selectedNode).classed('selected', true);

            // Highlight connected edges
            linkElements.classed('highlighted', link =>
                link.source.id === d.id || link.target.id === d.id
            );

            // Show details
            showNodeDetails(d);
        }

        function showNodeDetails(node) {
            const container = document.getElementById('selected-node-info');

            let propsHtml = '';
            if (node.properties) {
                propsHtml = '<dl class="detail-props">';
                for (const [key, value] of Object.entries(node.properties)) {
                    if (value && value !== 'unknown') {
                        propsHtml += `<dt>${key}</dt><dd>${value}</dd>`;
                    }
                }
                propsHtml += '</dl>';
            }

            container.innerHTML = `
                <div class="detail-card">
                    <h4>${node.label}</h4>
                    <span class="type-badge" style="background: ${typeColors[node.type] || typeColors.unknown}">${node.type}</span>
                    ${propsHtml}
                </div>
            `;
        }

        function updateVisibility(searchQuery = '') {
            nodeElements.style('opacity', d => {
                const matchesSearch = !searchQuery || d.label.toLowerCase().includes(searchQuery);
                const matchesType = activeTypes.has(d.type);
                return matchesSearch && matchesType ? 1 : 0.1;
            });

            linkElements.style('opacity', d => {
                const sourceVisible = activeTypes.has(d.source.type);
                const targetVisible = activeTypes.has(d.target.type);
                return sourceVisible && targetVisible ? 0.6 : 0.05;
            });
        }

        function resetZoom() {
            svg.transition()
                .duration(750)
                .call(zoom.transform, d3.zoomIdentity);
        }

        function toggleLabels() {
            showLabels = !showLabels;
            nodeElements.selectAll('text').style('opacity', showLabels ? 1 : 0);
        }

        // Handle window resize
        window.addEventListener('resize', () => {
            if (svg) {
                const container = document.getElementById('graph-container');
                svg.attr('width', container.clientWidth)
                   .attr('height', container.clientHeight);

                if (simulation) {
                    simulation.force('center', d3.forceCenter(
                        container.clientWidth / 2,
                        container.clientHeight / 2
                    ));
                    simulation.alpha(0.3).restart();
                }
            }
        });

        // Load data on page load
        loadData();
    </script>
</body>
</html>
"##.to_string()
}
