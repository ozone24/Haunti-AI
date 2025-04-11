pragma circom 2.1.6;

/* 
Haunti Training Verifier Circuit
Proves correct execution of model training steps without revealing:
- Raw training data
- Model parameters
- Gradient values
*/

include "node_modules/circomlib/circuits/comparators.circom";
include "node_modules/circomlib/circuits/safearithmetic.circom";

// Core ML Operations
template MatrixMultiply(m, n, p) {
    signal input A[m][n];
    signal input B[n][p];
    signal output C[m][p];
    
    for (var i = 0; i < m; i++) {
        for (var j = 0; j < p; j++) {
            C[i][j] <== 0;
            for (var k = 0; k < n; k++) {
                C[i][j] += A[i][k] * B[k][j];
            }
        }
    }
}

template ReLU() {
    signal input in;
    signal output out;
    
    component lt = LessEqThan(64);
    lt.in[0] <== in;
    lt.in[1] <== 0;
    
    out <== in * (1 - lt.out);
}

// Training Step Verification
template ForwardPass(layer_sizes) {
    var num_layers = layer_sizes.length - 1;
    signal input weights[num_layers][][];  // Weights per layer
    signal input biases[num_layers][];
    signal input activations[0][];          // Input data
    
    signal output output_activations[num_layers + 1][][];
    
    // Input layer
    output_activations[0] <== activations;
    
    for (var l = 0; l < num_layers; l++) {
        var in_size = layer_sizes[l];
        var out_size = layer_sizes[l + 1];
        
        component matmul = MatrixMultiply(1, in_size, out_size);
        matmul.A <== output_activations[l];
        matmul.B <== weights[l];
        
        component add_bias[out_size];
        component relu[out_size];
        
        for (var j = 0; j < out_size; j++) {
            add_bias[j] = SafeAdd(64);
            add_bias[j].in[0] <== matmul.C[0][j];
            add_bias[j].in[1] <== biases[l][j];
            
            relu[j] = ReLU();
            relu[j].in <== add_bias[j].out;
            
            output_activations[l + 1][0][j] <== relu[j].out;
        }
    }
}

template BackwardPass(layer_sizes, learning_rate) {
    var num_layers = layer_sizes.length - 1;
    signal input weights[num_layers][][];
    signal input gradients[num_layers][][];
    signal input old_biases[num_layers][];
    
    signal output new_weights[num_layers][][];
    signal output new_biases[num_layers][];
    
    for (var l = num_layers - 1; l >= 0; l--) {
        var in_size = layer_sizes[l];
        var out_size = layer_sizes[l + 1];
        
        // Weight update: W_new = W_old - η * gradient
        component weight_update[in_size][out_size];
        for (var i = 0; i < in_size; i++) {
            for (var j = 0; j < out_size; j++) {
                weight_update[i][j] = SafeSub(64);
                weight_update[i][j].in[0] <== weights[l][i][j];
                weight_update[i][j].in[1] <== gradients[l][i][j] * learning_rate;
                new_weights[l][i][j] <== weight_update[i][j].out;
            }
        }
        
        // Bias update: b_new = b_old - η * gradient_mean
        component bias_mean[out_size];
        component bias_update[out_size];
        for (var j = 0; j < out_size; j++) {
            bias_mean[j] = SafeDiv(64);
            bias_mean[j].in[0] <== gradients[l][0][j];  // Mean over batch
            bias_mean[j].in[1] <== 1;  // Placeholder for actual batch size
            
            bias_update[j] = SafeSub(64);
            bias_update[j].in[0] <== old_biases[l][j];
            bias_update[j].in[1] <== bias_mean[j].out * learning_rate;
            new_biases[l][j] <== bias_update[j].out;
        }
    }
}

// Main Training Verifier
component Main {public [input_hash, output_hash]} = 
    TrainingProcedureVerifier(layer_sizes, learning_rate, batch_size);
