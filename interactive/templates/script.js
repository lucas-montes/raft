/**
 * @fileoverview Raft visualization frontend with WebSocket communication
 * @author Lucas
 */

/**
 * @typedef {Object} Node
 * @property {'leader'|'follower'|'candidate'} role - The node's current role
 * @property {number} term - Current term number
 * @property {number} commitIndex - Index of highest log entry known to be committed
 * @property {number} x - X coordinate for visualization
 * @property {number} y - Y coordinate for visualization
 * @property {'active'|'joining'|'disconnected'} status - Node connection status
 * @property {string} addr - Node address (IP:port)
 */

/**
 * @typedef {Object} ServerLog
 * @property {string} action - The action that occurred
 * @property {string} timestamp - When the action occurred
 * @property {string} addr - Address of the node
 * @property {string} name - Span name
 */

/**
 * @typedef {Object} MessageAnimation
 * @property {number} startX - Starting X coordinate
 * @property {number} startY - Starting Y coordinate
 * @property {number} endX - Ending X coordinate
 * @property {number} endY - Ending Y coordinate
 * @property {number} currentX - Current X coordinate
 * @property {number} currentY - Current Y coordinate
 * @property {'vote'|'append'|'heartbeat'} type - Type of message
 * @property {number} progress - Animation progress (0-1)
 * @property {number} duration - Animation duration in milliseconds
 * @property {number} startTime - Animation start time
 */

/**
 * @typedef {Object} LogData
 * @property {string} action - The action performed
 * @property {string} timestamp - Timestamp of the action
 * @property {Object} span - Span information
 * @property {string} span.addr - Node address
 * @property {string} span.name - Span name
 */

// WebSocket connection to Axum backend
/** @type {WebSocket} */
const ws = new WebSocket('ws://localhost:3000/ws');

// Store cluster state
/** @type {Object<string, Node>} */
let nodes = {}; // { node_id: { role, term, commit_index, x, y, status, addr } }

/** @type {ServerLog[]} */
let serverLogs = []; // Store server log messages

/** @type {MessageAnimation[]} */
let messageAnimations = []; // Store active message animations

// Canvas for message animations
/** @type {HTMLCanvasElement|null} */
let canvas = null;

/** @type {CanvasRenderingContext2D|null} */
let ctx = null;

/**
 * Initialize canvas after DOM loads
 */
document.addEventListener('DOMContentLoaded', () => {
    canvas = /** @type {HTMLCanvasElement} */ (document.getElementById('message-canvas'));
    ctx = canvas.getContext('2d');
    resizeCanvas();
    startAnimationLoop();
    initializeCrudTabs();
    initializeCrudForms();
});

// WebSocket event handlers
ws.onopen = () => {
    console.log('Connected to backend');
    updateConnectionStatus('Connected');
};

/**
 * Handle incoming WebSocket messages
 * @param {MessageEvent} event - The WebSocket message event
 */
ws.onmessage = (event) => {
    /** @type {LogData} */
    const data = JSON.parse(event.data);

    // Handle ServerLogType messages (the only messages from backend)
    if (data.action) {
        handleServerLog(data);
    } else {
        console.log('Unknown message type:', data);
    }
};

/**
 * Handle WebSocket errors
 * @param {Event} error - The error event
 */
ws.onerror = (error) => {
    console.error('WebSocket error:', error);
    updateConnectionStatus('Error: Disconnected');
};

ws.onclose = () => {
    updateConnectionStatus('Disconnected');
};

/**
 * Handle server log messages from backend
 * @param {LogData} logData - The log data received from server
 */
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

/**
 * Update node role based on server logs
 * @param {string} addr - Node address
 * @param {'leader'|'follower'|'candidate'} role - New role for the node
 */
function updateNodeFromLog(addr, role) {
    if (!nodes[addr]) {
        initializeNodeFromLog(addr);
    }
    nodes[addr].role = role;
    renderNodes();
    updateStatus();
}

/**
 * Initialize a new node from log data
 * @param {string} addr - Node address
 */
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

/**
 * Animate message between nodes
 * @param {string} fromAddr - Source node address
 * @param {string|null} toAddr - Target node address (null for broadcast)
 * @param {'vote'|'append'|'heartbeat'} messageType - Type of message to animate
 */
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

/**
 * Create a single message animation
 * @param {Node} fromNode - Source node
 * @param {Node} toNode - Target node
 * @param {'vote'|'append'|'heartbeat'} messageType - Type of message
 */
function createMessageAnimation(fromNode, toNode, messageType) {
    /** @type {MessageAnimation} */
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

/**
 * Start the animation loop for message animations
 */
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

/**
 * Render server logs in UI
 */
function renderServerLogs() {
    const serverLogsDiv = /** @type {HTMLElement|null} */ (document.getElementById('server-logs'));
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

/**
 * Render nodes as DOM elements
 */
function renderNodes() {
    const nodesDiv = /** @type {HTMLElement|null} */ (document.getElementById('nodes'));
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

/**
 * Update connection status display
 * @param {string} status - Status message to display
 */
function updateConnectionStatus(status) {
    const statusDiv = /** @type {HTMLElement|null} */ (document.getElementById('status'));
    if (statusDiv) {
        statusDiv.textContent = status;
    }
}

/**
 * Update cluster status information
 */
function updateStatus() {
    const statusDiv = /** @type {HTMLElement|null} */ (document.getElementById('status'));
    if (!statusDiv) return;

    const leader = Object.entries(nodes).find(([id, node]) => node.role === 'leader')?.[0] || 'None';
    const nodeCount = Object.keys(nodes).length;
    const quorum = Math.floor(nodeCount / 2) + 1;

    const connectionStatus = ws.readyState === WebSocket.OPEN ? 'Connected' : 'Disconnected';
    statusDiv.textContent = `${connectionStatus} | Nodes: ${nodeCount} | Leader: ${leader} | Quorum: ${quorum}`;
}

/**
 * Update remove node dropdown options
 */
function updateRemoveNodeOptions() {
    const select = /** @type {HTMLSelectElement|null} */ (document.getElementById('remove-node-select'));
    if (!select) return;

    select.innerHTML = '<option value="">Select node to remove</option>';
    Object.keys(nodes).forEach(id => {
        const option = document.createElement('option');
        option.value = id;
        option.textContent = id;
        select.appendChild(option);
    });
}

/**
 * Initialize canvas size based on container
 */
function resizeCanvas() {
    const canvas = /** @type {HTMLCanvasElement|null} */ (document.getElementById('message-canvas'));
    const nodesDiv = /** @type {HTMLElement|null} */ (document.getElementById('nodes'));
    if (canvas && nodesDiv) {
        canvas.width = nodesDiv.clientWidth;
        canvas.height = nodesDiv.clientHeight;
    }
}

// Event Listeners
// Handle add node form submission
const addNodeForm = /** @type {HTMLFormElement|null} */ (document.getElementById('add-node-form'));
if (addNodeForm) {
    addNodeForm.addEventListener('submit', (e) => {
        e.preventDefault();
        const addrInput = /** @type {HTMLInputElement} */ (document.getElementById('node-addr'));
        const portInput = /** @type {HTMLInputElement} */ (document.getElementById('node-port'));

        const addr = addrInput.value;
        const port = portInput.value;

        if (addr && port) {
            ws.send(JSON.stringify({ action: 'addNode', addr, port }));
            addNodeForm.reset();
        }
    });
}

// Handle remove node button click
const removeNodeBtn = /** @type {HTMLButtonElement|null} */ (document.getElementById('remove-node-btn'));
if (removeNodeBtn) {
    removeNodeBtn.addEventListener('click', () => {
        const select = /** @type {HTMLSelectElement} */ (document.getElementById('remove-node-select'));
        const id = select.value;
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

window.addEventListener('resize', resizeCanvas);

// Initial setup
document.addEventListener('DOMContentLoaded', () => {
    resizeCanvas();
    updateStatus();
});

function initializeCrudTabs() {
    const tabs = document.querySelectorAll('.tab');
    const tabContents = document.querySelectorAll('.tab-content');

    tabs.forEach(tab => {
        tab.addEventListener('click', () => {
            // Remove active class from all tabs and contents
            tabs.forEach(t => t.classList.remove('active'));
            tabContents.forEach(tc => tc.classList.remove('active'));

            // Add active class to clicked tab
            tab.classList.add('active');

            // Show corresponding content
            const tabId = tab.getAttribute('data-tab') + '-tab';
            const content = document.getElementById(tabId);
            if (content) {
                content.classList.add('active');
            }
        });
    });
}


function initializeCrudForms() {
    // Create form
    const createForm = document.getElementById('create-form');
    if (createForm) {
        createForm.addEventListener('submit', async (e) => {
            e.preventDefault();
            const data = document.getElementById('create-data').value;

            try {
                JSON.parse(data); // Validate JSON
                await sendCrudRequest('Create', { data });
                createForm.reset();
            } catch (error) {
                showCrudResponse('Invalid JSON format', false);
            }
        });
    }

    // Read form
    const readForm = document.getElementById('read-form');
    if (readForm) {
        readForm.addEventListener('submit', async (e) => {
            e.preventDefault();
            const id = document.getElementById('read-id').value;
            await sendCrudRequest('Read', { id });
        });
    }

    // Update form
    const updateForm = document.getElementById('update-form');
    if (updateForm) {
        updateForm.addEventListener('submit', async (e) => {
            e.preventDefault();
            const id = document.getElementById('update-id').value;
            const data = document.getElementById('update-data').value;

            try {
                JSON.parse(data); // Validate JSON
                await sendCrudRequest('Update', { id, data });
                updateForm.reset();
            } catch (error) {
                showCrudResponse('Invalid JSON format', false);
            }
        });
    }

    // Delete form
    const deleteForm = document.getElementById('delete-form');
    if (deleteForm) {
        deleteForm.addEventListener('submit', async (e) => {
            e.preventDefault();
            const id = document.getElementById('delete-id').value;
            await sendCrudRequest('Delete', { id });
            deleteForm.reset();
        });
    }
}



async function sendCrudRequest(operation, payload) {
    try {
        showCrudResponse('Sending request...', true);

        // Send CRUD request through WebSocket
        const message = {
            action: 'crudOperation',
            operation,
            ...payload
        };

        ws.send(JSON.stringify(message));

        // Note: In a real implementation, you'd want to handle the response
        // For now, we'll just show a success message
        setTimeout(() => {
            showCrudResponse(`${operation.toUpperCase()} operation sent successfully`, true);
        }, 500);

    } catch (error) {
        showCrudResponse(`Error: ${error.message}`, false);
    }
}

function showCrudResponse(message, isSuccess) {
    const responseContainer = document.getElementById('crud-response');
    const responseContent = document.getElementById('crud-response-content');

    if (responseContainer && responseContent) {
        responseContainer.style.display = 'block';
        responseContainer.className = `response-container ${isSuccess ? 'response-success' : 'response-error'}`;
        responseContent.textContent = message;

        // Auto-hide after 5 seconds
        setTimeout(() => {
            if (!isSuccess || message.includes('sent successfully')) {
                responseContainer.style.display = 'none';
            }
        }, 5000);
    }
}
