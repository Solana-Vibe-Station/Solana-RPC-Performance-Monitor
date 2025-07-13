
let updating = false;
let previousValues = {};
let responseTimeChart;
let hiddenDatasets = {};

const colors = [
    'rgba(255, 99, 132, 1)',
    'rgba(54, 162, 235, 1)',
    'rgba(255, 206, 86, 1)',
    'rgba(75, 192, 192, 1)',
    'rgba(153, 102, 255, 1)',
    'rgba(255, 159, 64, 1)'
];

const nicknameColorMap = {};

function updateWithFade(elementId, value) {
    const element = document.getElementById(elementId);
    element.style.opacity = '0';
    setTimeout(() => {
        element.textContent = value;
        element.style.opacity = '1';
    }, 150);
}

function updateLeaderboard(elementId, data, formatFn, title) {
    const element = document.getElementById(elementId);
    let html = `<h3 class="text-lg font-semibold mb-2">${title}</h3>`;

    if (title === 'Highest Slots') {
        data = data.map(entry => {
            return entry.latency_ms > 200 ? { ...entry, value: entry.value - 1 } : entry;
        });
        data.sort((a, b) => {
            if (b.value === a.value) {
                return a.total_latency_ms - b.total_latency_ms;
            }
            return b.value - a.value;
        });
    } else if (title === 'Fastest Response Times') {
        data.sort((a, b) => {
            // Use latency_ms if it exists, else fallback to total_latency_ms
            const aLatency = a.latency_ms !== undefined ? a.latency_ms : a.total_latency_ms;
            const bLatency = b.latency_ms !== undefined ? b.latency_ms : b.total_latency_ms;
            return aLatency - bLatency;
        });
    }

    data.forEach((entry, index) => {
        let timeStr = '';
        if (entry.timestamp) {
            const date = new Date(entry.timestamp);
            timeStr = date.toLocaleTimeString() + '.' + date.getMilliseconds().toString().padStart(3, '0');
        }
        const key = elementId + index;
        const isNewRecord = previousValues[key] !== entry.value;
        const highlightClass = isNewRecord ? 'new-record' : '';
        previousValues[key] = entry.value;

        html += `
<div class="flex items-center justify-between mb-2 ${highlightClass} ${index === 0 ? 'text-green-600 font-medium' : ''} p-2 rounded">
  <span class="flex-grow">${index + 1}. ${formatFn(entry)}</span>
  <span class="text-sm text-gray-500">${timeStr}</span>
</div>
`;
    });

    element.innerHTML = html;
}


function updateYAxisMax() {
    const visibleDatasets = responseTimeChart.data.datasets.filter(dataset => !dataset.hidden);
    const maxLatency = Math.max(...visibleDatasets.flatMap(dataset => dataset.data.map(point => point.y)));
    responseTimeChart.options.scales.y.max = maxLatency * 1.1;
    responseTimeChart.update();
}

async function fetchData() {
    if (updating) return;
    updating = true;
    requestCount++;

    document.getElementById('updateIndicator').classList.add('updating');

    try {

        let url = '/api/metrics?';

        const response = await fetch(url);
        const [data, consensus] = await response.json();

        console.log('Fetched data:', data);
        console.log('Consensus data:', consensus);

        // Update consensus information
        updateWithFade('fastestRPC', `${consensus.fastest_rpc} (${consensus.fastest_latency}ms)`);
        updateWithFade('slowestRPC', `${consensus.slowest_rpc} (${consensus.slowest_latency}ms)`);
        updateWithFade('consensusBlockhash', consensus.consensus_blockhash);
        updateWithFade('consensusSlot', consensus.consensus_slot);
        updateWithFade('consensusPercentage', consensus.consensus_percentage.toFixed(1) + '%');
        updateWithFade('averageLatency', ((consensus.slowest_latency - consensus.fastest_latency) / consensus.slowest_latency * 100).toFixed(2) + '%');
        updateWithFade('slotDifference', consensus.slot_skew);

        // Update leaderboards
        updateLeaderboard('latencyLeaderboard', consensus.latency_leaderboard,
            entry => `${entry.nickname}: ${entry.latency_ms}ms`,
            'Fastest Response Times');
        // Calculate average response time per endpoint from raw data
        const nicknames = [...new Set(data.map(item => item.nickname))].sort();
        const avgResponseTimes = nicknames.map(nickname => {
            const endpointData = data.filter(item => item.nickname === nickname);
            const totalLatency = endpointData.reduce((sum, item) => sum + item.total_latency_ms, 0);
            const avgLatency = endpointData.length > 0 ? totalLatency / endpointData.length : 0;
            return {
                nickname,
                averageLatency: avgLatency
            };
        });

        // Sort by lowest average response time first
        avgResponseTimes.sort((a, b) => a.averageLatency - b.averageLatency);


        // Update leaderboard with average response times
        updateLeaderboard('slotLeaderboard', avgResponseTimes,
            entry => `${entry.nickname}: ${entry.averageLatency.toFixed(2)}ms`,
            'Average Response Times');

        // Update graph
        const datasets = nicknames.map((nickname, index) => {
            if (!nicknameColorMap[nickname]) {
                nicknameColorMap[nickname] = colors[index % colors.length];
            }
            const color = nicknameColorMap[nickname];
            return {
                label: nickname,
                data: data.filter(item => item.nickname === nickname).map(item => ({
                    x: new Date(item.timestamp),
                    y: item.latency_ms
                })),
                borderColor: color,
                backgroundColor: color.replace('1)', '0.2)'),
                borderWidth: 2,
                pointRadius: 0,
                hidden: hiddenDatasets[nickname] || false
            };
        });

        if (responseTimeChart) {
            responseTimeChart.data.datasets = datasets;
            responseTimeChart.update();
        } else {
            const ctx = document.getElementById('responseTimeChart').getContext('2d');
            responseTimeChart = new Chart(ctx, {
                type: 'line',
                data: {
                    datasets: datasets
                },
                options: {
                    responsive: true,
                    maintainAspectRatio: false,
                    plugins: {
                        legend: {
                            display: false
                        },
                        title: {
                            display: true,
                            text: 'Response Times'
                        }
                    },
                    scales: {
                        x: {
                            type: 'time',
                            time: {
                                unit: 'minute'
                            },
                            title: {
                                display: true,
                                text: 'Time'
                            }
                        },
                        y: {
                            title: {
                                display: true,
                                text: 'Response Time (ms)'
                            },
                            beginAtZero: true,
                            max: Math.max(...datasets.flatMap(dataset => dataset.data.map(point => point.y))) * 1.1
                        }
                    }
                }
            });
        }

        // Debugging statements
        console.log('Chart Data:', responseTimeChart.data.datasets);
        responseTimeChart.data.datasets.forEach(dataset => {
            console.log(`Dataset ${dataset.label} hidden:`, dataset.hidden);
        });
        console.log('Hidden Datasets:', hiddenDatasets);

        // Update legend
        const legend = document.getElementById('legend');
        legend.innerHTML = '';
        nicknames.forEach((nickname, index) => {
            const color = nicknameColorMap[nickname];
            const legendItem = document.createElement('div');
            legendItem.className = 'legend-item flex items-center';
            legendItem.innerHTML = `
<div class="w-4 h-4 mr-2" style="background-color: ${color};"></div>
<span>${nickname}</span>
`;
            legendItem.onclick = () => {
                const dataset = responseTimeChart.data.datasets.find(ds => ds.label === nickname);
                console.log(`Toggling visibility for ${nickname}`);
                dataset.hidden = !dataset.hidden;
                hiddenDatasets[nickname] = dataset.hidden;
                updateYAxisMax();
                console.log(`Toggled ${nickname} hidden state to:`, dataset.hidden);
            };
            legend.appendChild(legendItem);
        });

        document.getElementById('status').textContent = `Last updated: ${new Date().toLocaleTimeString()}`;
    } catch (error) {
        console.error('Error fetching data:', error);
        document.getElementById('status').textContent = 'Error updating data';
    } finally {
        updating = false;
        document.getElementById('updateIndicator').classList.remove('updating');
    }
}

// Track request rate
let requestCount = 0;
let lastRateUpdate = Date.now();

function updateRequestRate() {
    const now = Date.now();
    const elapsed = (now - lastRateUpdate) / 1000;
    const rate = requestCount / elapsed;
    document.getElementById('requestRate').textContent =
        `${rate.toFixed(1)} requests/sec`;
    requestCount = 0;
    lastRateUpdate = now;
}

let autoRefresh = true;
let refreshInterval;

function toggleRefresh() {
    autoRefresh = !autoRefresh;
    const button = document.getElementById('refreshToggle');
    const indicator = document.getElementById('updateIndicator');

    if (autoRefresh) {
        refreshInterval = setInterval(fetchData, 5000);
        button.textContent = 'Pause Updates';
        button.classList.remove('bg-green-500');
        button.classList.add('bg-red-500');
        indicator.classList.remove('hidden');
    } else {
        clearInterval(refreshInterval);
        button.textContent = 'Resume Updates';
        button.classList.remove('bg-red-500');
        button.classList.add('bg-green-500');
        indicator.classList.add('hidden');
    }
}

// Initialize
document.addEventListener('DOMContentLoaded', () => {
    fetchData();
    refreshInterval = setInterval(fetchData, 5000);
    setInterval(updateRequestRate, 1000);
});