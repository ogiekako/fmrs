import { Position } from './position/Position';

function App() {
  return <Position onChange={board => {
    console.log(board)
  }} />;
}

export default App;
