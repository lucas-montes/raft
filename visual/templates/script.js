// WebSocket connection to Axum backend
const ws = new WebSocket('ws://localhost:3000/ws');

// Store cluster state
let nodes = {}; // { node_id: { role, term, commit_index, x, y, status, addr } }
let serverLogs = []; // Store server log messages
let messageAnimations = []; // Store active message animations

// Canvas for message animations
let canvas, ctx;

// Initialize canvas after DOM loads
document.addEventListener('DOMContentLoaded', () => {
    canvas = document.getElementById('message-canvas');
    ctx = canvas.getContext('2d');
    resizeCanvas();
    startAnimationLoop();
});

// WebSocket event handlers
ws.onopen = () => {
    console.log('Connected to backend');
    updateConnectionStatus('Connected');
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
    updateConnectionStatus('Error: Disconnected');
};

ws.onclose = () => {
    updateConnectionStatus('Disconnected');
};

// Handle server log messages from backend
function handleServerLog(logData) {
    const { action, timestamp, span } = logData;


    if (!action || !span) {
        console.error('Invalid log data:', logData);
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
            initializeNodeFromLog(span.addr);
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
            animateMessage(span.addr, null, 'append');
            break;
        case 'sendHeartbeat':
            animateMessage(span.addr, null, 'heartbeat');
            break;
        case 'sendVotes':
            animateMessage(span.addr, null, 'vote');
            break;
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
        initializeNodeFromLog(addr);
    }
    nodes[addr].role = role;
    renderNodes();
    updateStatus();
}

// Initialize a new node from log data
function initializeNodeFromLog(addr) {
    if (!nodes[addr]) {
        // Calculate position in a circle for better layout
        const nodeCount = Object.keys(nodes).length;
        const angle = (nodeCount * 2 * Math.PI) / Math.max(5, nodeCount + 1);
        const centerX = 300;
        const centerY = 200;
        const radius = 120;

        const x = centerX + Math.cos(angle) * radius;
        const y = centerY + Math.sin(angle) * radius;

        nodes[addr] = {
            role: 'follower',
            term: 0,
            commitIndex: 0,
            x: x,
            y: y,
            status: 'active',
            addr: addr
        };
        renderNodes();
        updateRemoveNodeOptions();
        updateStatus();
    }
}

// Animate message between nodes
function animateMessage(fromAddr, toAddr, messageType) {
    const fromNode = nodes[fromAddr];
    if (!fromNode) return;

    // If no specific target, send to all other nodes
    if (!toAddr) {
        Object.keys(nodes).forEach(addr => {
            if (addr !== fromAddr) {
                createMessageAnimation(fromNode, nodes[addr], messageType);
            }
        });
    } else {
        const toNode = nodes[toAddr];
        if (toNode) {
            createMessageAnimation(fromNode, toNode, messageType);
        }
    }
}

// Create a single message animation
function createMessageAnimation(fromNode, toNode, messageType) {
    const message = {
        startX: fromNode.x + 40,
        startY: fromNode.y + 40,
        endX: toNode.x + 40,
        endY: toNode.y + 40,
        currentX: fromNode.x + 40,
        currentY: fromNode.y + 40,
        type: messageType,
        progress: 0,
        duration: 1000, // 1 second
        startTime: Date.now()
    };

    messageAnimations.push(message);
}

// Animation loop
function startAnimationLoop() {
    function animate() {
        if (ctx && canvas) {
            ctx.clearRect(0, 0, canvas.width, canvas.height);

            const currentTime = Date.now();
            messageAnimations = messageAnimations.filter(message => {
                const elapsed = currentTime - message.startTime;
                message.progress = Math.min(elapsed / message.duration, 1);

                // Easing function for smooth animation
                const easeProgress = 1 - Math.pow(1 - message.progress, 3);

                message.currentX = message.startX + (message.endX - message.startX) * easeProgress;
                message.currentY = message.startY + (message.endY - message.startY) * easeProgress;

                // Draw message ball
                ctx.beginPath();
                ctx.arc(message.currentX, message.currentY, 6, 0, 2 * Math.PI);

                switch (message.type) {
                    case 'vote':
                        ctx.fillStyle = '#ffc107';
                        ctx.shadowColor = '#ffc107';
                        break;
                    case 'append':
                        ctx.fillStyle = '#20c997';
                        ctx.shadowColor = '#20c997';
                        break;
                    case 'heartbeat':
                        ctx.fillStyle = '#007bff';
                        ctx.shadowColor = '#007bff';
                        break;
                    default:
                        ctx.fillStyle = '#6c757d';
                        ctx.shadowColor = '#6c757d';
                }

                ctx.shadowBlur = 10;
                ctx.fill();
                ctx.shadowBlur = 0;

                return message.progress < 1;
            });
        }

        requestAnimationFrame(animate);
    }
    animate();
}

// Render server logs in UI
function renderServerLogs() {
    const serverLogsDiv = document.getElementById('server-logs');
    if (!serverLogsDiv) return;

    // Keep the header and create/update content
    const existingHeader = serverLogsDiv.querySelector('h3');
    serverLogsDiv.innerHTML = '';
    if (existingHeader) {
        serverLogsDiv.appendChild(existingHeader);
    } else {
        const header = document.createElement('h3');
        header.textContent = 'Server Logs';
        serverLogsDiv.appendChild(header);
    }

    // Show most recent logs first
    const recentLogs = serverLogs.slice(-50).reverse();

    recentLogs.forEach(log => {
        const logDiv = document.createElement('div');
        logDiv.className = `server-log-entry ${log.action.toLowerCase()}`;

        // Parse timestamp
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
            <span class="node-addr">${log.addr}</span>
        `;
        serverLogsDiv.appendChild(logDiv);
    });
}

// Render nodes as <div> elements
function renderNodes() {
    const nodesDiv = document.getElementById('nodes');
    if (!nodesDiv) return;

    // Clear existing nodes but keep canvas
    const existingCanvas = nodesDiv.querySelector('#message-canvas');
    nodesDiv.innerHTML = '';
    if (existingCanvas) {
        nodesDiv.appendChild(existingCanvas);
    }

    Object.entries(nodes).forEach(([id, node]) => {
        const div = document.createElement('div');
        div.className = `node ${node.role} ${node.status === 'joining' ? 'joining' : ''}`;
        div.style.left = node.x + 'px';
        div.style.top = node.y + 'px';
        div.textContent = node.addr;
        div.title = `${node.addr} - ${node.role.toUpperCase()}`;
        nodesDiv.appendChild(div);
    });
}

// Update connection status
function updateConnectionStatus(status) {
    const statusDiv = document.getElementById('status');
    if (statusDiv) {
        statusDiv.textContent = status;
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

    select.innerHTML = '<option value="">Select node to remove</option>';
    Object.keys(nodes).forEach(id => {
        const option = document.createElement('option');
        option.value = id;
        option.textContent = id;
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
document.addEventListener('DOMContentLoaded', () => {
    resizeCanvas();
    updateStatus();
});
