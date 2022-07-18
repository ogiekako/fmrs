const path = require('path');
const HtmlWebpackPlugin = require('html-webpack-plugin');
const WasmPackPlugin = require("@wasm-tool/wasm-pack-plugin");

module.exports = {
    mode: 'production',
    entry: './app/src/index.tsx',
    output: {
        path: path.join(__dirname, 'dist'),
        filename: 'main.js',
    },
    module: {
        rules: [
            {
                test: /\.tsx?$/,
                use: [
                    // {
                    //     loader: 'babel-loader',
                    //     options: { presets: ['@babel/preset-env', '@babel/react'] },
                    // },
                    {
                        loader: 'ts-loader',
                        options: {
                            configFile: path.resolve(__dirname, 'app/tsconfig.json'),
                        },
                    },
                ],
            },
            {
                test: /\.css$/,
                use: ['style-loader', 'css-loader'],
            }
        ],
    },
    plugins: [
        new HtmlWebpackPlugin({
            template: './app/public/index.html'
        }),
        new WasmPackPlugin({
            crateDirectory: path.resolve(__dirname, "rust/wasm"),
            outDir: path.resolve(__dirname, 'dist/pkg'),
        }),
    ],
    experiments: {
        asyncWebAssembly: true,
    },
    performance: {
        hints: false,
    },
    devServer: {
        static: {
            directory: path.join(__dirname, 'dist'),
        },
        port: 3000,
    },
    resolve: {
        extensions: ['.ts', '.tsx', '.js', '.json'],
    },
    target: 'web',
};
