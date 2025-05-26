// WebSocket connection to Axum backend
const ws = new WebSocket('ws://localhost:3000/ws');

// Store cluster state
let nodes = {}; // { node_id: { role, term, commit_index, x, y, status, name } }
let serverLogs = []; // Store server log messages

// Canvas for message animations
const canvas = document.getElementById('message-canvas');
const ctx = canvas.getContext('2d');

// WebSocket event handlers
ws.onopen = () => {
    console.log('Connected to backend');
    document.getElementById('status').textContent = 'Connected';
};

ws.onmessage = (event) => {
    const data = JSON.parse(event.data);

    // Handle ServerLogType messages (the only messages from backend)
    if (data.action) {
        handleServerLog(data);
    } else {
        console.log('Unknown message type:', data);
    }
};

ws.onerror = (error) => {
    console.error('WebSocket error:', error);
    document.getElementById('status').textContent = 'Error: Disconnected';
};

ws.onclose = () => {
    document.getElementById('status').textContent = 'Disconnected';
};

// Handle server log messages from backend
function handleServerLog(logData) {
    const { action, timestamp, span } = logData;

    if (!action || !span) {
        console.error('Invalid log data:', logEntry);
        return;
    }

    // Add to server logs array
    serverLogs.push({
        action,
        timestamp,
        addr: span.addr,
        name: span.name
    });

    // Keep only last 100 logs to prevent memory issues
    if (serverLogs.length > 100) {
        serverLogs = serverLogs.slice(-100);
    }

    // Update the logs display
    renderServerLogs();

    // Handle specific actions for node state updates
    switch (action) {
        case 'starting':
            // Initialize node when it starts
            initializeNodeFromLog(span.addr, span.name);
            break;
        case 'becomeLeader':
            updateNodeFromLog(span.addr, 'leader');
            break;
        case 'becomeFollower':
            updateNodeFromLog(span.addr, 'follower');
            break;
        case 'becomeCandidate':
            updateNodeFromLog(span.addr, 'candidate');
            break;
        case 'sendAppendEntries':
        case 'sendHeartbeat':
        case 'sendVotes':
        case 'receiveAppendEntries':
        case 'receiveVote':
        case 'startTransaction':
        case 'votingFor':
        case 'create':
            // These actions don't change node state but are logged
            console.log(`${span.addr} performed: ${action}`);
            break;
    }
}

// Update node role based on server logs
function updateNodeFromLog(addr, role) {
    if (!nodes[addr]) {
        initializeNodeFromLog(addr, addr);
    }
    nodes[addr].role = role;
    renderNodes();
    updateStatus();
}

// Initialize a new node from log data
function initializeNodeFromLog(addr, name) {
    if (!nodes[addr]) {
        // Generate random position for new nodes
        const x = Math.random() * 400 + 50;
        const y = Math.random() * 300 + 50;

        nodes[addr] = {
            role: 'follower',
            term: 0,
            commitIndex: 0,
            x: x,
            y: y,
            status: 'active',
            name: name
        };
        renderNodes();
        updateRemoveNodeOptions();
        updateStatus();
    }
}

// Render server logs in UI
function renderServerLogs() {
    let serverLogsDiv = document.getElementById('server-logs');

    // Create the server logs container if it doesn't exist
    if (!serverLogsDiv) {
        serverLogsDiv = document.createElement('div');
        serverLogsDiv.id = 'server-logs';
        serverLogsDiv.className = 'server-logs-container';

        // Insert after the status div
        const statusDiv = document.getElementById('status');
        if (statusDiv && statusDiv.parentNode) {
            statusDiv.parentNode.insertBefore(serverLogsDiv, statusDiv.nextSibling);
        } else {
            document.body.appendChild(serverLogsDiv);
        }
    }

    serverLogsDiv.innerHTML = '<h3>Server Logs</h3>';

    // Show most recent logs first
    const recentLogs = serverLogs.slice(-20).reverse();

    recentLogs.forEach(log => {
        const logDiv = document.createElement('div');
        logDiv.className = `server-log-entry ${log.action.toLowerCase()}`;

        // Parse timestamp if it's a string
        let timeDisplay = log.timestamp;
        try {
            const date = new Date(log.timestamp);
            timeDisplay = date.toLocaleTimeString();
        } catch (e) {
            // If timestamp parsing fails, use as is
        }

        logDiv.innerHTML = `
            <span class="timestamp">${timeDisplay}</span>
            <span class="action">${log.action}</span>
            <span class="node">${log.name}</span>
        `;
        serverLogsDiv.appendChild(logDiv);
    });
}

// Render nodes as <div> elements
function renderNodes() {
    const nodesDiv = document.getElementById('nodes');
    if (!nodesDiv) return;

    nodesDiv.innerHTML = '<canvas id="message-canvas" style="position: absolute; top: 0; left: 0; width: 100%; height: 100%;"></canvas>';

    Object.entries(nodes).forEach(([id, node]) => {
        const div = document.createElement('div');
        div.className = `node ${node.role} ${node.status === 'joining' ? 'joining' : ''}`;
        div.style.left = node.x + 'px';
        div.style.top = node.y + 'px';
        div.textContent = `${node.name || id} (${node.role})`;
        nodesDiv.appendChild(div);
    });

    // Update canvas reference after re-creating it
    const newCanvas = document.getElementById('message-canvas');
    if (newCanvas) {
        newCanvas.width = nodesDiv.clientWidth;
        newCanvas.height = nodesDiv.clientHeight;
    }
}

// Update cluster status
function updateStatus() {
    const statusDiv = document.getElementById('status');
    if (!statusDiv) return;

    const leader = Object.entries(nodes).find(([id, node]) => node.role === 'leader')?.[0] || 'None';
    const nodeCount = Object.keys(nodes).length;
    const quorum = Math.floor(nodeCount / 2) + 1;

    const connectionStatus = ws.readyState === WebSocket.OPEN ? 'Connected' : 'Disconnected';
    statusDiv.textContent = `${connectionStatus} | Nodes: ${nodeCount} | Leader: ${leader} | Quorum: ${quorum}`;
}

// Update remove node dropdown
function updateRemoveNodeOptions() {
    const select = document.getElementById('remove-node-select');
    if (!select) return;

    select.innerHTML = '<option value="">Select node</option>';
    Object.keys(nodes).forEach(id => {
        const option = document.createElement('option');
        option.value = id;
        option.textContent = `${nodes[id].name || id} (${id})`;
        select.appendChild(option);
    });
}

// Handle add node form submission
const addNodeForm = document.getElementById('add-node-form');
if (addNodeForm) {
    addNodeForm.addEventListener('submit', (e) => {
        e.preventDefault();
        const addr = document.getElementById('node-addr').value;
        const port = document.getElementById('node-port').value;

        if (addr && port) {
            ws.send(JSON.stringify({ action: 'addNode', addr, port }));
            addNodeForm.reset();
        }
    });
}

// Handle remove node button click
const removeNodeBtn = document.getElementById('remove-node-btn');
if (removeNodeBtn) {
    removeNodeBtn.addEventListener('click', () => {
        const id = document.getElementById('remove-node-select').value;
        if (id) {
            ws.send(JSON.stringify({ action: 'removeNode', id }));
            // Remove from local state immediately for better UX
            delete nodes[id];
            renderNodes();
            updateRemoveNodeOptions();
            updateStatus();
        }
    });
}

// Initialize canvas size
function resizeCanvas() {
    const canvas = document.getElementById('message-canvas');
    const nodesDiv = document.getElementById('nodes');
    if (canvas && nodesDiv) {
        canvas.width = nodesDiv.clientWidth;
        canvas.height = nodesDiv.clientHeight;
    }
}

window.addEventListener('resize', resizeCanvas);
// Initial setup
resizeCanvas();
updateStatus();
