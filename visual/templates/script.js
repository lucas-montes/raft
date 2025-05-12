// const socket = new WebSocket('ws://localhost:3000/ws');

// socket.addEventListener('open', function (event) {
//     socket.send('Hello Server!');
// });

// socket.addEventListener('message', function (event) {
//     console.log('Message from server ', event.data);
// });


// setTimeout(() => {
//     const obj = { hello: "world" };
//     const blob = new Blob([JSON.stringify(obj, null, 2)], {
//       type: "application/json",
//     });
//     console.log("Sending blob over websocket");
//     socket.send(blob);
// }, 1000);

// setTimeout(() => {
//     socket.send('About done here...');
//     console.log("Sending close over websocket");
//     socket.close(3000, "Crash and Burn!");
// }, 3000);

  // WebSocket connection to Axum backend
  const ws = new WebSocket('ws://localhost:3000/ws');

  // Store cluster state
  let nodes = {}; // { node_id: { role, term, commit_index, x, y, status } }
  let logs = {}; // { node_id: [{ index, term, committed }] }
  let messages = []; // [{ from, to, rpc, timestamp }]

  // Canvas for message animations
  const canvas = document.getElementById('message-canvas');
  const ctx = canvas.getContext('2d');


  // WebSocket event handlers
  ws.onopen = () => {
      console.log('Connected to backend');
      // Request initial cluster state
      ws.send(JSON.stringify({ action: 'getState' }));
  };

  ws.onmessage = (event) => {
      const data = JSON.parse(event.data);
      if (data.type === 'state_change') {
          // Update node role, term, etc.
          updateNode(data.node_id, data.role, data.term, data.commit_index, data.status);
      } else if (data.type === 'log_update') {
          // Update node logs
          updateLogs(data.node_id, data.entries, data.commit_index);
      } else if (data.type === 'message') {
          // Add message for animation
          addMessage(data.from, data.to, data.rpc);
      } else if (data.type === 'cluster_state') {
          // Initialize or update entire cluster
          initializeCluster(data.nodes, data.logs);
      }
  };

  ws.onerror = (error) => {
      console.error('WebSocket error:', error);
      document.getElementById('status').textContent = 'Error: Disconnected';
  };

  // ws.onclose = () => {
  //     document.getElementById('status').textContent = 'Disconnected';
  // };

  // Update node in UI
  function updateNode(nodeId, role, term, commitIndex, status) {
      nodes[nodeId] = nodes[nodeId] || { x: 0, y: 0 };
      nodes[nodeId].role = role;
      nodes[nodeId].term = term;
      nodes[nodeId].commitIndex = commitIndex;
      nodes[nodeId].status = status || 'active';
      renderNodes();
      updateStatus();
  }

  // Update logs in UI
  function updateLogs(nodeId, entries, commitIndex) {
      logs[nodeId] = entries.map(entry => ({
          index: entry.index,
          term: entry.term,
          committed: entry.index <= commitIndex
      }));
      renderLogs();
  }

  // Add message for animation
  function addMessage(from, to, rpc) {
      messages.push({ from, to, rpc, timestamp: Date.now() });
      renderMessages();
  }

  // Initialize cluster state
  function initializeCluster(nodesData, logsData) {
      nodes = nodesData;
      logs = logsData;
      renderNodes();
      renderLogs();
      updateRemoveNodeOptions();
  }

  // Render nodes as <div> elements
  function renderNodes() {
      const nodesDiv = document.getElementById('nodes');
      nodesDiv.innerHTML = '<canvas id="message-canvas" style="position: absolute; top: 0; left: 0; width: 100%; height: 100%;"></canvas>';
      Object.entries(nodes).forEach(([id, node]) => {
          const div = document.createElement('div');
          div.className = `node ${node.role} ${node.status === 'joining' ? 'joining' : ''}`;
          div.style.left = node.x + 'px';
          div.style.top = node.y + 'px';
          div.textContent = `${id} (T${node.term})`;
          nodesDiv.appendChild(div);
      });
  }

  // Render logs as <div> elements
  function renderLogs() {
      const logsDiv = document.getElementById('logs');
      logsDiv.innerHTML = '';
      Object.entries(logs).forEach(([nodeId, entries]) => {
          const nodeDiv = document.createElement('div');
          nodeDiv.innerHTML = `<h3>${nodeId}</h3>`;
          entries.forEach(entry => {
              const entryDiv = document.createElement('div');
              entryDiv.className = `log-entry ${entry.committed ? 'committed' : ''}`;
              entryDiv.textContent = `Index: ${entry.index}, Term: ${entry.term}`;
              nodeDiv.appendChild(entryDiv);
          });
          logsDiv.appendChild(nodeDiv);
      });
  }

  // Render messages (animations on canvas)
  function renderMessages() {
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      messages = messages.filter(msg => Date.now() - msg.timestamp < 2000); // Keep messages for 2s
      messages.forEach(msg => {
          const fromNode = nodes[msg.from];
          const toNode = nodes[msg.to];
          if (fromNode && toNode) {
              ctx.beginPath();
              ctx.moveTo(fromNode.x + 25, fromNode.y + 25);
              ctx.lineTo(toNode.x + 25, toNode.y + 25);
              ctx.strokeStyle = 'black';
              ctx.stroke();
              // Add arrowhead or label (simplified)
          }
      });
      requestAnimationFrame(renderMessages);
  }

  // Update cluster status
  function updateStatus() {
      const leader = Object.entries(nodes).find(([id, node]) => node.role === 'leader')?.[0] || 'None';
      const quorum = Math.floor(Object.keys(nodes).length / 2) + 1;
      document.getElementById('status').textContent = `Leader: ${leader} | Quorum: ${quorum}`;
  }

  // Update remove node dropdown
  function updateRemoveNodeOptions() {
      const select = document.getElementById('remove-node-select');
      select.innerHTML = '<option value="">Select node</option>';
      Object.keys(nodes).forEach(id => {
          const option = document.createElement('option');
          option.value = id;
          option.textContent = id;
          select.appendChild(option);
      });
  }

  // Handle add node form submission
  document.getElementById('add-node-form').addEventListener('submit', (e) => {
      e.preventDefault();
      const addr = document.getElementById('node-addr').value;
      const port = document.getElementById('node-port').value;
      ws.send(JSON.stringify({ action: 'addNode', addr, port }));
      document.getElementById('add-node-form').reset();
  });

  // Handle remove node button click
  document.getElementById('remove-node-btn').addEventListener('click', () => {
      const id = document.getElementById('remove-node-select').value;
      if (id) {
          ws.send(JSON.stringify({ action: 'removeNode', id }));
      }
  });

  // Initialize canvas size
  function resizeCanvas() {
      canvas.width = document.getElementById('nodes').clientWidth;
      canvas.height = document.getElementById('nodes').clientHeight;
  }
  window.addEventListener('resize', resizeCanvas);
  resizeCanvas();
